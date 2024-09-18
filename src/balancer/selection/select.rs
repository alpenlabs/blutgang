use crate::{
    rpc::types::RouteGroup,
    Rpc,
};
use std::time::SystemTime;

#[derive(Debug)]
pub struct RpcIndexed<'a> {
    rpc: &'a mut Rpc,
    idx: usize,
}

impl<'a> RpcIndexed<'a> {
    fn inner(&self) -> &Rpc {
        self.rpc
    }

    fn inner_mut(&mut self) -> &mut Rpc {
        self.rpc
    }
}

// Generic entry point fn to select the next rpc and return its position
pub fn pick(list: &mut [Rpc], route_group: &RouteGroup) -> (Rpc, Option<usize>) {
    let mut filtered_list = list
        .iter_mut()
        .enumerate()
        .filter_map(|(idx, rpc)| {
            if rpc.group == *route_group {
                Some(RpcIndexed { rpc, idx })
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    // If len is 1, return the only element
    if filtered_list.len() == 1 {
        return (list[0].clone(), Some(0));
    } else if filtered_list.is_empty() {
        return (Rpc::default(), None);
    }

    let picked_idx = algo(&mut filtered_list);
    let picked = &filtered_list[picked_idx];
    (picked.inner().clone(), Some(picked.idx))
}

// Sorting algo
pub fn argsort(data: &[RpcIndexed]) -> Vec<usize> {
    let mut indices = (0..data.len()).collect::<Vec<usize>>();

    // Use sort_by_cached_key with a closure that compares latency
    // Uses pdqsort and does not allocate so should be fast
    indices.sort_unstable_by_key(|&index| data[index].inner().status.latency as u64);

    indices
}

// Selection algorithms
//
// Selected via features. selection-weighed-round-robin is a default feature.
// In order to have custom algos, you must add and enable the feature,
// as well as modify the cfg of the default algo to accomodate your new feature.
//
#[cfg(all(
    feature = "selection-weighed-round-robin",
    not(feature = "selection-random"),
    not(feature = "old-weighted-round-robin"),
))]
fn algo(list: &mut [RpcIndexed]) -> usize {
    // Sort by latency
    let indices = argsort(list);

    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Failed to get current time")
        .as_micros();

    // Picks the second fastest one rpc that meets our requirements
    // Also take into account min_delta_time

    // Set fastest rpc as default
    let mut choice = indices[0];
    let mut choice_consecutive = 0;
    for i in indices.iter().rev() {
        if list[*i].inner().max_consecutive > list[*i].inner().consecutive
            && (time - list[*i].inner().last_used > list[*i].inner().min_time_delta)
        {
            choice = *i;
            choice_consecutive = list[*i].inner().consecutive;
        }

        // remove consecutive
        list[*i].inner_mut().consecutive = 0;
    }

    // If no RPC has been selected, fall back to the fastest RPC
    list[choice].inner_mut().consecutive = choice_consecutive + 1;
    list[choice].inner_mut().last_used = time;
    choice
}

#[cfg(all(
    feature = "selection-weighed-round-robin",
    feature = "selection-random"
))]
fn algo(list: &mut [RpcIndexed]) -> usize {
    use rand::Rng;

    let mut rng = rand::thread_rng();
    let index = rng.gen_range(0..list.len());
    index
}

#[cfg(all(
    feature = "selection-weighed-round-robin",
    feature = "old-weighted-round-robin",
))]
fn algo(list: &mut [RpcIndexed]) -> usize {
    // Sort by latency
    let indices = argsort(list);

    // Picks the second fastest one if the fastest one has maxed out
    if list[indices[0]].inner().max_consecutive <= list[indices[0]].inner().consecutive {
        list[indices[1]].inner_mut().consecutive = 1;
        list[indices[0]].inner_mut().consecutive = 0;
        return indices[1];
    }

    list[indices[0]].inner_mut().consecutive += 1;
    indices[0]
}

// Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_algo() {
        let mut rpc1 = Rpc::default();
        let mut rpc2 = Rpc::default();
        let mut rpc3 = Rpc::default();

        rpc1.status.latency = 1.0;
        rpc2.status.latency = 2.0;
        rpc3.status.latency = 3.0;

        let mut v = vec![rpc2, rpc3, rpc1];
        let vx = v.clone();
        let vi = v
            .iter_mut()
            .enumerate()
            .map(|(idx, rpc)| RpcIndexed { rpc, idx })
            .collect::<Vec<_>>();
        let i = argsort(&vi);
        assert_eq!(i, &[2, 0, 1]);
        assert_eq!(vi[0].inner().get_url(), vx[0].get_url());
    }

    // Test picking the fastest RPC
    // Change the latencies of the other ones to simulate
    // real network fluctuations.
    #[test]
    fn test_pick() {
        let mut rpc1 = Rpc::default();
        let mut rpc2 = Rpc::default();
        let mut rpc3 = Rpc::default();

        rpc1.status.latency = 3.0;
        rpc1.max_consecutive = 10;
        rpc1.min_time_delta = 100;

        rpc2.status.latency = 7.0;
        rpc2.max_consecutive = 10;
        rpc2.min_time_delta = 100;

        rpc3.status.latency = 5.0;
        rpc3.max_consecutive = 10;
        rpc3.min_time_delta = 100;

        let mut rpc_list = vec![rpc1, rpc2, rpc3];

        let (rpc, index) = pick(&mut rpc_list, &RouteGroup::default());
        println!("rpc: {:?}", rpc);
        assert_eq!(rpc.status.latency, 3.0);
        assert_eq!(index, Some(0));

        rpc_list[0].status.latency = 10000.0;

        let (rpc, index) = pick(&mut rpc_list, &RouteGroup::default());
        println!("rpc index: {:?}", index);
        assert_eq!(rpc.status.latency, 5.0);
        assert_eq!(index, Some(2));

        rpc_list[2].status.latency = 100000.0;

        let (rpc, index) = pick(&mut rpc_list, &RouteGroup::default());
        assert_eq!(rpc.status.latency, 7.0);
        assert_eq!(index, Some(1));
    }

    // Test max_delay when picking rpcs
    #[test]
    fn test_pick_max_delay() {
        let mut rpc1 = Rpc::default();
        let mut rpc2 = Rpc::default();
        let mut rpc3 = Rpc::default();

        rpc1.status.latency = 3.0;
        rpc1.max_consecutive = 10;
        rpc1.min_time_delta = 1701357164371770;
        rpc1.last_used = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Failed to get current time")
            .as_micros();

        rpc2.status.latency = 7.0;
        rpc2.max_consecutive = 10;
        rpc2.min_time_delta = 1;

        rpc3.status.latency = 5.0;
        rpc3.max_consecutive = 10;
        rpc3.min_time_delta = 10000000;

        let mut rpc_list = vec![rpc1, rpc2, rpc3];

        // Pick rpc3 becauese rpc1 does not meet last used requirements
        let (rpc, index) = pick(&mut rpc_list, &RouteGroup::default());
        println!("rpc: {:?}", rpc);
        assert_eq!(rpc.status.latency, 5.0);
        assert_eq!(index, Some(2));

        // pick rpc2 because rpc3 was just used
        let (rpc, index) = pick(&mut rpc_list, &RouteGroup::default());
        println!("rpc index: {:?}", index);
        assert_eq!(rpc.status.latency, 7.0);
        assert_eq!(index, Some(1));
    }
}
