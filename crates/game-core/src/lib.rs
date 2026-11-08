//! 共享游戏核心逻辑：组件、系统、网络协议、地图数据
//!
//! 客户端和服务端共用同一份 `game-core`，保证逻辑一致。

pub mod abilities;
pub mod components;
pub mod gameplay;
pub mod map;
pub mod math;
pub mod net;
pub mod systems;
pub mod visibility;

#[cfg(test)]
mod tests {
    #[test]
    fn test_workspace_compiles() {
        // 确保 game-core 能编译
    }
}
