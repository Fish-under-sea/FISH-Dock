// 合成插件后端（示例）：订阅主 Dock 的文件拖入，收到路径后开一个自己的外部窗口。
// 这不是官方插件，只是 SDK 的最小可运行样例：
//   · 真正的 Dock 里：本文件即插件清单登记的 backend.entry（node 运行时）。
//   · 没有 Dock 时：harness.mjs 用同一份 plugin() 在进程内跑完整协议往返（不需要子进程）。
import { createPlugin } from '../../node/plugin.mjs';

const PLUGIN_ID = 'acme.notes';
// 最近一次拖入的文件数量：示例只记数量，不把完整路径写进日志（宿主也是这样）。
export function plugin(options = {}) {
  let dropped = 0;
  let panel = null;
  let subscription = null;
  let stopWindows = null;
  let stopDrop = null;
  return createPlugin({
    id: PLUGIN_ID,
    ...options,
    async activate(context) {
      // ① 显式申请主 Dock 的拖入（宿主侧独占：被别的插件占着会明确报冲突）。
      subscription = await context.fileDrop.subscribe({ scope: 'dock' });
      // ② 本插件的窗口事件：ready / closed / anchorChanged / fileDrop。
      stopWindows = context.windows.onEvent(event => {
        if (event.event === 'closed') panel = null;
      });
      stopDrop = context.fileDrop.onEvent(async event => {
        if (event.phase !== 'drop') return;
        dropped += Array.isArray(event.paths) ? event.paths.length : 0;
        // ③ 收到路径 → 更新自己的业务状态 → 按需开外部窗口（宿主不替插件画 UI）。
        const opening = panel
          ? context.windows.update(panel, { height: 260 })
          : context.windows.open({ key: 'panel', page: 'panel', title: '我的暂存', width: 420, height: 260 })
              .then(handle => { panel = handle; return handle; });
        // SDK 按序等待监听器；完成本次开窗后才处理停用，避免迟到结果复活 panel。
        await opening.catch(() => { panel = null; });
      });
    },
    async deactivate() {
      // ④ 宿主在 deactivate 前已撤销窗口、订阅和 Hook 会话；这里只清本地监听器。
      stopWindows?.();
      stopDrop?.();
      stopWindows = stopDrop = null;
      subscription = null;
      panel = null;
    },
    commands: {
      // 页面桥或宿主的业务调用可以查询示例状态。
      status: async () => ({ dropped, panel }),
      show: async (_params, context) => {
        if (panel) return { handle: panel };
        panel = await context.windows.open({ key: 'panel', page: 'panel', title: '我的暂存' });
        return { handle: panel };
      },
    },
  });
}