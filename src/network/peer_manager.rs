use libp2p::PeerId;
use std::collections::HashMap;
use std::time::{Duration, Instant};
pub const INVALID_BLOCK_PENALTY: i32 = -10;
pub const INVALID_TX_PENALTY: i32 = -5;
pub const OVERSIZED_MESSAGE_PENALTY: i32 = -3;
pub const TIMEOUT_PENALTY: i32 = -15;
pub const SLOW_SYNC_PENALTY: i32 = -5;
pub const INVALID_HANDSHAKE_PENALTY: i32 = -20;
pub const GOOD_BEHAVIOR_REWARD: i32 = 1;
pub const BAN_THRESHOLD: i32 = -100;
pub const BAN_DURATION: Duration = Duration::from_secs(3600);
pub const MAX_SCORE: i32 = 100;
pub const MIN_SCORE: i32 = -99;
pub const MAX_MSG_BURST: f64 = 20.0;
pub const MSG_REFILL_RATE: f64 = 5.0;
#[derive(Debug, Clone)]
pub struct PeerScore {
    pub score: i32,
    pub banned_until: Option<Instant>,
    pub invalid_blocks: u32,
    pub invalid_txs: u32,
    pub valid_contributions: u32,
    pub last_seen: Option<Instant>,
    pub rate_tokens: f64,
    pub rate_last_refill: Instant,
    pub vote_tokens: f64,
    pub blob_tokens: f64,
    pub handshaked: bool,
}
impl Default for PeerScore {
    fn default() -> Self {
        PeerScore {
            score: 0,
            banned_until: None,
            invalid_blocks: 0,
            invalid_txs: 0,
            valid_contributions: 0,
            last_seen: None,
            rate_tokens: MAX_MSG_BURST,
            rate_last_refill: Instant::now(),
            vote_tokens: 10.0,
            blob_tokens: 5.0,
            handshaked: false,
        }
    }
}
impl PeerScore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_banned(&self) -> bool {
        if let Some(until) = self.banned_until {
            Instant::now() < until
        } else {
            false
        }
    }
    pub fn refill_tokens(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.rate_last_refill).as_secs_f64();

        self.rate_tokens = (self.rate_tokens + elapsed * MSG_REFILL_RATE).min(MAX_MSG_BURST);

        self.vote_tokens = (self.vote_tokens + elapsed * 2.0).min(20.0);

        self.blob_tokens = (self.blob_tokens + elapsed * 0.5).min(10.0);

        self.rate_last_refill = now;
    }
    pub fn consume_token(&mut self) -> bool {
        self.refill_tokens();
        if self.rate_tokens >= 1.0 {
            self.rate_tokens -= 1.0;
            true
        } else {
            false
        }
    }
    pub fn ban_remaining(&self) -> Option<Duration> {
        self.banned_until.and_then(|until| {
            let now = Instant::now();
            if now < until {
                Some(until - now)
            } else {
                None
            }
        })
    }
}
pub struct PeerManager {
    peers: HashMap<PeerId, PeerScore>,
}
impl PeerManager {
    pub fn new() -> Self {
        PeerManager {
            peers: HashMap::new(),
        }
    }
    fn get_or_create(&mut self, peer_id: &PeerId) -> &mut PeerScore {
        self.peers.entry(*peer_id).or_insert_with(PeerScore::new)
    }
    pub fn check_rate_limit(&mut self, peer_id: &PeerId) -> bool {
        let score = self.get_or_create(peer_id);
        if !score.consume_token() {
            score.score = (score.score + OVERSIZED_MESSAGE_PENALTY).max(MIN_SCORE);
            if score.score <= BAN_THRESHOLD {
                let until = Instant::now() + BAN_DURATION;
                score.banned_until = Some(until);
            }
            return false;
        }
        true
    }

    pub fn check_vote_rate_limit(&mut self, peer_id: &PeerId) -> bool {
        let score = self.get_or_create(peer_id);
        score.refill_tokens();
        if score.vote_tokens >= 1.0 {
            score.vote_tokens -= 1.0;
            true
        } else {
            score.score = (score.score - 1).max(MIN_SCORE);
            false
        }
    }

    pub fn check_blob_rate_limit(&mut self, peer_id: &PeerId) -> bool {
        let score = self.get_or_create(peer_id);
        score.refill_tokens();
        if score.blob_tokens >= 1.0 {
            score.blob_tokens -= 1.0;
            true
        } else {
            score.score = (score.score - 5).max(MIN_SCORE);
            false
        }
    }
    pub fn report_invalid_block(&mut self, peer_id: &PeerId) {
        let score = self.get_or_create(peer_id);
        score.invalid_blocks += 1;
        score.score = score.score + INVALID_BLOCK_PENALTY;
        score.last_seen = Some(Instant::now());
        if score.score <= BAN_THRESHOLD {
            self.ban_peer(peer_id);
        }
    }
    pub fn report_invalid_tx(&mut self, peer_id: &PeerId) {
        let score = self.get_or_create(peer_id);
        score.invalid_txs += 1;
        score.score = score.score + INVALID_TX_PENALTY;
        score.last_seen = Some(Instant::now());
        if score.score <= BAN_THRESHOLD {
            self.ban_peer(peer_id);
        }
    }
    pub fn report_oversized_message(&mut self, peer_id: &PeerId) {
        let score = self.get_or_create(peer_id);
        score.score = score.score + OVERSIZED_MESSAGE_PENALTY;
        score.last_seen = Some(Instant::now());
        if score.score <= BAN_THRESHOLD {
            self.ban_peer(peer_id);
        }
    }
    pub fn report_bad_behavior(&mut self, peer_id: &PeerId) {
        let score = self.get_or_create(peer_id);
        score.score = (score.score - 10).max(MIN_SCORE);
        score.last_seen = Some(Instant::now());
        if score.score <= BAN_THRESHOLD {
            self.ban_peer(peer_id);
        }
    }
    pub fn report_good_behavior(&mut self, peer_id: &PeerId) {
        let score = self.get_or_create(peer_id);
        score.valid_contributions += 1;
        score.score = (score.score + GOOD_BEHAVIOR_REWARD).min(MAX_SCORE);
        score.last_seen = Some(Instant::now());
    }
    pub fn ban_peer(&mut self, peer_id: &PeerId) {
        let score = self.get_or_create(peer_id);
        score.banned_until = Some(Instant::now() + BAN_DURATION);
        println!("Peer {} banned for {:?}", peer_id, BAN_DURATION);
    }
    pub fn is_banned(&self, peer_id: &PeerId) -> bool {
        self.peers
            .get(peer_id)
            .map(|s| s.is_banned())
            .unwrap_or(false)
    }
    pub fn get_score(&self, peer_id: &PeerId) -> i32 {
        self.peers.get(peer_id).map(|s| s.score).unwrap_or(0)
    }
    pub fn is_handshaked(&self, peer_id: &PeerId) -> bool {
        self.peers
            .get(peer_id)
            .map(|s| s.handshaked)
            .unwrap_or(false)
    }
    pub fn set_handshaked(&mut self, peer_id: &PeerId, status: bool) {
        let score = self.get_or_create(peer_id);
        score.handshaked = status;
    }
    pub fn get_peer_info(&self, peer_id: &PeerId) -> Option<&PeerScore> {
        self.peers.get(peer_id)
    }
    pub fn unban_peer(&mut self, peer_id: &PeerId) {
        if let Some(score) = self.peers.get_mut(peer_id) {
            score.banned_until = None;
            score.score = 0;
        }
    }
    pub fn cleanup_expired_bans(&mut self) {
        let now = Instant::now();
        for score in self.peers.values_mut() {
            if let Some(until) = score.banned_until {
                if now >= until {
                    score.banned_until = None;
                    score.score = 0;
                }
            }
        }
    }
    pub fn get_banned_peers(&self) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter(|(_, score)| score.is_banned())
            .map(|(id, _)| *id)
            .collect()
    }
    pub fn report_timeout(&mut self, peer_id: &PeerId) {
        let score = self.get_or_create(peer_id);
        score.score = (score.score + TIMEOUT_PENALTY).max(MIN_SCORE);
        score.last_seen = Some(Instant::now());
        if score.score <= BAN_THRESHOLD {
            self.ban_peer(peer_id);
        }
    }
    pub fn report_slow_sync(&mut self, peer_id: &PeerId) {
        let score = self.get_or_create(peer_id);
        score.score = (score.score + SLOW_SYNC_PENALTY).max(MIN_SCORE);
        score.last_seen = Some(Instant::now());
        if score.score <= BAN_THRESHOLD {
            self.ban_peer(peer_id);
        }
    }
    pub fn report_invalid_handshake(&mut self, peer_id: &PeerId) {
        let score = self.get_or_create(peer_id);
        score.score = (score.score + INVALID_HANDSHAKE_PENALTY).max(MIN_SCORE);
        score.last_seen = Some(Instant::now());
        if score.score <= BAN_THRESHOLD {
            self.ban_peer(peer_id);
        }
    }
    pub fn get_best_peers(&self, n: usize) -> Vec<PeerId> {
        let mut scored: Vec<_> = self.peers.iter().filter(|(_, s)| !s.is_banned()).collect();
        scored.sort_by(|a, b| b.1.score.cmp(&a.1.score));
        scored.into_iter().take(n).map(|(id, _)| *id).collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn test_peer_id() -> PeerId {
        PeerId::random()
    }
    #[test]
    fn test_new_peer_has_zero_score() {
        let manager = PeerManager::new();
        let peer = test_peer_id();
        assert_eq!(manager.get_score(&peer), 0);
    }
    #[test]
    fn test_invalid_block_penalty() {
        let mut manager = PeerManager::new();
        let peer = test_peer_id();
        manager.report_invalid_block(&peer);
        assert_eq!(manager.get_score(&peer), INVALID_BLOCK_PENALTY);
    }
    #[test]
    fn test_good_behavior_reward() {
        let mut manager = PeerManager::new();
        let peer = test_peer_id();
        manager.report_good_behavior(&peer);
        assert_eq!(manager.get_score(&peer), GOOD_BEHAVIOR_REWARD);
    }
    #[test]
    fn test_peer_gets_banned() {
        let mut manager = PeerManager::new();
        let peer = test_peer_id();
        for _ in 0..11 {
            manager.report_invalid_block(&peer);
        }
        assert!(manager.is_banned(&peer));
        assert!(manager.get_score(&peer) <= BAN_THRESHOLD);
    }
    #[test]
    fn test_unban_peer() {
        let mut manager = PeerManager::new();
        let peer = test_peer_id();
        manager.ban_peer(&peer);
        assert!(manager.is_banned(&peer));
        manager.unban_peer(&peer);
        assert!(!manager.is_banned(&peer));
        assert_eq!(manager.get_score(&peer), 0);
    }
    #[test]
    fn test_score_capped_at_max() {
        let mut manager = PeerManager::new();
        let peer = test_peer_id();
        for _ in 0..200 {
            manager.report_good_behavior(&peer);
        }
        assert_eq!(manager.get_score(&peer), MAX_SCORE);
    }
}
