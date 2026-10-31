//! 匹配队列算法

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct QueuedPlayer {
    pub player_id: String,
    pub hero_id: u32,
    pub mmr: u32,
    pub joined_at: std::time::Instant,
}

#[derive(Debug, Default,Clone)]
pub struct MatchQueue {
    queue: Arc<RwLock<Vec<QueuedPlayer>>>,
    player_matches: Arc<RwLock<HashMap<String, (u64, String)>>>, // player_id -> (match_id, server_addr)
}

impl MatchQueue {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(RwLock::new(Vec::new())),
            player_matches: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn join(&self, player_id: String, hero_id: u32, mmr: u32) -> u32 {
        let mut queue = self.queue.write().await;
        
        // 检查是否已在队列中
        if queue.iter().any(|p| p.player_id == player_id) {
            return queue.iter().position(|p| p.player_id == player_id).map(|p| p as u32 + 1).unwrap_or(0);
        }
        
        let player = QueuedPlayer {
            player_id: player_id.clone(),
            hero_id,
            mmr,
            joined_at: std::time::Instant::now(),
        };
        
        queue.push(player);
        
        // 尝试匹配
        self.try_match().await;
        
        queue.iter().position(|p| p.player_id == player_id).map(|p| p as u32 + 1).unwrap_or(0)
    }
    
    pub async fn leave(&self, player_id: &str) {
        let mut queue = self.queue.write().await;
        queue.retain(|p| p.player_id != player_id);
    }
    
    pub async fn get_status(&self, player_id: &str) -> (bool, Option<u64>, Option<String>) {
        let queue = self.queue.read().await;
        let in_queue = queue.iter().any(|p| p.player_id == player_id);
        
        let matches = self.player_matches.read().await;
        if let Some((match_id, server_addr)) = matches.get(player_id) {
            return (false, Some(*match_id), Some(server_addr.clone()));
        }
        
        (in_queue, None, None)
    }
    
    async fn try_match(&self) {
        let mut queue = self.queue.write().await;
        
        if queue.len() >= 6 {
            // 简单匹配：取前 6 个玩家
            let matched: Vec<_> = queue.drain(0..6).collect();
            
            let match_id = Uuid::new_v4().as_u128() as u64;
            let server_addr = "127.0.0.1:5000".to_string(); // TODO: 动态分配 battle-server
            
            let mut matches = self.player_matches.write().await;
            for player in matched {
                matches.insert(player.player_id, (match_id, server_addr.clone()));
            }
            
            tracing::info!("Match {} created with 6 players", match_id);
        }
    }
}