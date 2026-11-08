//! GX-01 端到端 / 合规测试宿主（`DEV-27` / `DEV-28`）。
//!
//! 该 crate 无运行时代码，仅用于承载同时依赖 client 与 server 的集成测试，
//! 从而解除 `client → server` 的反向依赖（`DN-34`）。
