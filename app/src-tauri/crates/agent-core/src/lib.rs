// 中性事件：一个会话「是什么状态」的机器可读说法，不带 mood / seen / 停留多久
// （那些是宿主的展示层，state.rs 的 Look 与 view）。
//
// 谁用：
//   - 宿主 state.rs 的 apply 把一条消息交给 AgentEvent::of_message，交给 agent_hub
//     的有界事件环（/v1/events）；
//   - events_codex.rs 用 codex::turn_origin 过滤**内部轮**（T-I）；
//   - 八类 agent 的适配器各自的行为特征测试与这里的测试对同一条原始事件断言。
//
// 不在这里的：registry（会话表）、asks（等回答的请求）、日志行、mood、i18n 文案、
// 读会话记录（DSH 的 zstd、Codex 的 rollout）——那些要么是宿主的展示层，要么需要
// 磁盘 I/O，留在宿主适配器里（方案 §2.2）。
pub mod codex;
pub mod event;

pub use event::{Agent, AgentEvent, Doing, EventError, Origin, Outcome};