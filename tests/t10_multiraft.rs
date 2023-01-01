use std::collections::HashMap;

use smol_raft::proto::ConfState;
use smol_raft::proto::HardState;
use smol_raft::proto::RaftGroupManagementMessage;
use smol_raft::proto::RaftGroupManagementMessageType;
use smol_raft::proto::ReplicaMetadata;
use smol_raft::proto::Snapshot;
use smol_raft::storage::MemStorage;
use smol_raft::storage::MultiRaftMemoryStorage;
use smol_raft::storage::MultiRaftStorage;
use smol_raft::storage::RaftStorage;
use smol_raft::LocalTransport;
use smol_raft::MultiRaft;
use smol_raft::MultiRaftConfig;
use smol_raft::MultiRaftMessageSender;
use tokio::sync::watch;

type FixtureMultiRaft = MultiRaft<
    MultiRaftMessageSender,
    LocalTransport<MultiRaftMessageSender>,
    MemStorage,
    MultiRaftMemoryStorage,
>;

pub struct FixtureCluster {
    storages: Vec<MultiRaftMemoryStorage>,
    multirafts: Vec<FixtureMultiRaft>,
    groups: HashMap<u64, Vec<u64>>, // track group which nodes, group_id -> nodes
}

impl FixtureCluster {
    pub fn make(num: u64, stop: watch::Receiver<bool>) -> FixtureCluster {
        let mut multirafts = vec![];
        let mut storages = vec![];
        for n in 0..num {
            let node_id = n + 1;
            let store_id = n + 1;
            let config = MultiRaftConfig {
                election_tick: 2,
                heartbeat_tick: 1,
                tick_interval: 100,
            };

            let transport = LocalTransport::new();
            let storage = MultiRaftMemoryStorage::new(node_id, store_id);
            storages.push(storage.clone());
            let multiraft = FixtureMultiRaft::new(config, node_id, store_id, transport, storage, stop.clone());
            multirafts.push(multiraft);
        }
        Self {
            storages,
            multirafts,
            groups: HashMap::new(),
        }
    }


    pub async fn make_group(&mut self, group_id: u64, first_node: u64, replica_num: usize) {
        let mut voters = vec![];
        let mut replicas = vec![];
        for i in 0..replica_num {
            let replica_id = (i + 1) as u64;
            let node_id = first_node + i as u64;
            voters.push(replica_id);
            replicas.push(ReplicaMetadata {
                node_id,
                replica_id,
                store_id: 0,
            });
        }

        for i in 0..replica_num {
            let node_index = first_node as usize + i;
            let replica_id = (i + 1) as u64;
            let storage = &self.storages[node_index];
            let gs = storage.group_storage(group_id, replica_id).await.unwrap();

            // init hardstate
            let mut hs = HardState::default();
            hs.commit = 1;
            hs.term = 1;
            gs.set_hardstate(hs).unwrap();

            // init confstate
            let mut cs = ConfState::default();
            cs.voters = voters.clone();
            gs.set_confstate(cs).unwrap();

            // apply snapshot
            let mut ss = Snapshot::default();
            ss.mut_metadata().mut_conf_state().voters = voters.clone();
            ss.mut_metadata().index = 1;
            ss.mut_metadata().term = 1;
            gs.apply_snapshot(ss).unwrap();

            let multiraft = &self.multirafts[node_index];
            let mut msg = RaftGroupManagementMessage::default();
            msg.set_msg_type(RaftGroupManagementMessageType::MsgInitialGroup);
            msg.group_id = group_id;
            msg.replica_id = replica_id;
            msg.replicas = replicas.clone();

            multiraft.initial_raft_group(msg).await.unwrap();

            match self.groups.get_mut(&group_id) {
                None => {
                    self.groups.insert(group_id, vec![node_index as u64]);
                }
                Some(nodes) => nodes.push(node_index as u64),
            };
        }
    }


    pub async fn check_elect(&mut self, leader_id: u64, group_id: u64) {
        // trigger an election for the replica in the group of the node where leader nodes.
        self.trigger_elect(leader_id, group_id).await;
        unimplemented!()
    }

    async fn trigger_elect(&self, node_id: u64, group_id: u64) {
        self.multirafts[node_id as usize].campagin(group_id).await
    }

    async fn wait_for_leader_elect(&self, node_id: u64) {

    }
}

impl FixtureCluster {}

#[tokio::test(flavor = "multi_thread")]
async fn test_initial_leader_elect() {
   for leader_id in 0..3 {
    let (stop_tx, stop_rx) = watch::channel(false);
    let mut cluster = FixtureCluster::make(3, stop_rx);
    let group_id = 1;
    cluster.make_group(group_id, 0, 3).await;

    cluster.check_elect(leader_id, group_id);
    stop_tx.send(true);
   } 
}
