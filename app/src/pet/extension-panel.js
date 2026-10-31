// 扩展管理状态以宿主结果为准，离线目录不遮住卸载和故障处理入口。
;(function () {
  const esc = value => String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c])
  const size = bytes => {
    let value = Math.max(0, Number(bytes) || 0), unit = 0
    const units = ['B', 'KB', 'MB', 'GB']
    while (value >= 1000 && unit < units.length - 1) { value /= 1000; unit++ }
    return `${Number(value.toFixed(unit ? 1 : 0))} ${units[unit]}`
  }
  // t44：下载量的显示格式。1024 进制自动进位（B→KB→MB→GB），B 不带小数，
  // 其余保留两位（如 512 B / 984.61 KB / 29.43 MB）。目录里的 `size` 用 1000
  // 那是「磁盘厂标」口径，这里是「用户看到的传输量」，二者不混用。
  const formatBytes = bytes => {
    let value = Math.max(0, Number(bytes) || 0)
    const units = ['B', 'KB', 'MB', 'GB', 'TB']
    let unit = 0
    while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++ }
    return unit === 0 ? `${Math.round(value)} B` : `${value.toFixed(2)} ${units[unit]}`
  }
  // t44：阶段显示文案——宿主 plugin_progress 的 phase 字符串（下载/校验/解包/提交）
  // 是协议，保持不变；这里只把它翻成「动词 + 中」。卸载没有宿主进度，由面板本地
  // 合成（phase '卸载'），文案同样落在这张表里。
  const PHASE_WORDS = {
    '下载': ['下载中', 'Downloading'],
    '校验': ['校验中', 'Verifying'],
    '解包': ['解包中', 'Unpacking'],
    '提交': ['提交中', 'Committing'],
    '卸载': ['卸载中', 'Uninstalling'],
    '切换来源': ['切换来源', 'Switching source'],
  }
  // t44：进度单调不减（不回弹）。换源（npm 官方 → 镜像 → GitHub）时宿主会从
  // 0 重新报 received，百分比就掉回去了；这里记住该 id 见过的峰值 peak，显示
  // 永远用 peak。新源的原始百分比 raw 还没追上峰值时，阶段显示「切换来源」，
  // 追上以后自然回到「下载中」。
  const mergeProgress = (prev, item) => {
    const total = Number(item?.total)
    const received = Number(item?.received) || 0
    const raw = total > 0 ? Math.min(100, Math.max(0, Math.floor((100 * received) / total))) : null
    const peak = Math.max(prev ? prev.peak : 0, raw ?? 0)
    const switching = !!prev && raw != null && prev.peak > 0 && raw < prev.peak - 1 && item?.phase === '下载'
    return { peak, raw, switching, total: total > 0 ? total : (prev?.total ?? null) }
  }
  // t47：粒子飞行包络（纯函数，单测用）。粒子 i 的目标列 slot=(i+0.5)/N（0..1，均匀铺满）；
  // in：pct 到 slot 时“抵达并入”，此前 FLIGHT_WINDOW 的进度窗口里在飞；
  // out：长条右缘 (1-pct) 越过该列（pct = 1-slot）时开始剥离向右飞。
  // 返回 null 表示这一帧不画它（还没出发 / 已并入 / 已消散）——已填区域因此保持干净。
  // 48 个粒子 × 0.25 窗口 ≈ 任意时刻只有 12 个在飞（对应“前沿附近 10~15 个”）。
  const FLIGHT_WINDOW = 0.25
  const particleFlight = (pct, index, count, mode = 'in') => {
    const slot = (index + 0.5) / count
    const trigger = mode === 'out' ? 1 - slot : slot
    const start = mode === 'out' ? trigger : trigger - FLIGHT_WINDOW
    const q = (pct - start) / FLIGHT_WINDOW
    if (!(q > 0 && q < 1)) return null
    return { q, slot, trigger }
  }
  const official = id => ['fish.feishu', 'fish.clash', 'fish.magpie'].includes(id)
  // 官方目录的作者（#22-4）：2.0.0 目录里 authors 总是存在（≥1，顺序即展示顺序），
  // 「A、B」由这里拼。旧目录的 author/maintainer 由宿主 plugin_store.rs 归一成同一
  // 结构，前端不再读那两个键。
  const authorsOf = entry => (Array.isArray(entry?.authors) ? entry.authors : []).filter(name => typeof name === 'string' && name.trim()).map(name => name.trim())
  // 插件卡的「随 Dock 启动」写的是注册表里的 autostart，与插件服务页同一份真相；
  // 旧注册表没有这一项时按开处理（plugin_install.rs desired_autostart）。
  const autostartOn = row => row?.plugin?.autostart !== false
  const newer = (candidate, current) => {
    const parse = value => /^\d+\.\d+\.\d+$/.test(String(value)) ? String(value).split('.').map(Number) : null
    const a = parse(candidate), b = parse(current)
    if (!a || !b) return false
    for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] > b[i]
    return false
  }
  // outdated：这一版要不要更新。只看版本号时，「同一版号重打的包」永远判不出更新
  // ——2.0.0 对 2.0.0 恒为 false，于是插件卡一直写「已安装」，用户点不到「更新」
  // （#f3「更新拿不到」）。所以版本相同但包摘要不同，同样算有更新；宿主注册表里的
  // digest 与官方目录的 sha256 都是小写十六进制，直接比。
  const outdated = (entry, plugin) => {
    if (!entry || !plugin?.manifest) return false
    if (newer(entry.version, plugin.manifest.version)) return true
    if (newer(plugin.manifest.version, entry.version)) return false
    const want = String(entry.sha256 || '').toLowerCase(), have = String(plugin.digest || '').toLowerCase()
    return !!want && !!have && want !== have
  }
  function create({ invoke, confirm, lang, redraw, openPage = () => {}, syncAuthFocus = async () => {}, deviceDialog = () => {} }) {
    let view = null, catalog = null, cached = false, storeError = '', error = '', busy = '', loaded = false, active = 'installed', node = '', checked = false
    // t29：安装/更新期间的下载进度（宿主 plugin_progress）。t38：按安装目标 id 分开，
    // 一个弹窗只对应一个 id；别人的取消/完成不会串台。
    let progress = [], progressTimer = 0
    // t44：进度弹窗的显示层状态——当前操作种类（install/uninstall/…）、每个 id
    // 见过的进度峰值（换源不回弹）、卸载没有宿主进度时的本地合成节拍、粒子动效。
    let busyKind = '', uninstallTicks = 0
    // 2026-11-01 用户要求：卸载的确认弹窗没点「确认」之前，一律不准提前渲染卸载进度。
    // run() 一开始就置 busy/busyKind，而 job() 第一步是 await confirm()，这段等待里
    // progressItems() 已经会合成卸载进度卡 —— 所以再加一道 actionStarted 闸。
    let actionStarted = false
    const progressSeen = new Map()
    let fxRaf = 0
    const fxParticles = new Map()
    let hits = [], thirdQuery = '', thirdNote = '', thirdSearched = false
    const text = (zh, en) => lang() === 'en' ? en : zh
    const fail = e => typeof e === 'string' ? e : e?.message || text('插件操作失败，请重试。', 'Extension operation failed. Please retry.')
    // t45：把宿主的「一句人话 + 逐源详情」JSON 解析出来；不是结构化错误就退回纯文本。
    const parseFailure = e => {
      const raw = typeof e === 'string' ? e : (e && e.message) || ''
      const trimmed = String(raw).trim()
      if (!trimmed.startsWith('{') || !trimmed.includes('"fishFailure"')) return null
      try { const obj = JSON.parse(trimmed); return obj && obj.fishFailure ? obj : null } catch { return null }
    }
    const failureOf = e => parseFailure(e) || fail(e)
    const stageText = stage => ({ download: text('下载失败', 'Download failed'), verify: text('校验失败', 'Verification failed'), uninstall: text('卸载失败', 'Uninstall failed') })[stage] || text('操作失败', 'Operation failed')
    const defaultHint = stage => ({ download: text('检查网络或代理后重试。', 'Check your network or proxy, then retry.'), verify: text('包可能损坏，请重试。', 'The package may be damaged; please retry.'), uninstall: text('请稍后重试。', 'Please retry later.') })[stage] || text('请稍后重试。', 'Please retry later.')
    const countText = n => n === 3 ? text('三个来源都没成功', 'All three sources failed') : text(`${n} 个来源都没成功`, `${n} sources failed`)
    // 失败块：一句人话 + 可展开详情（逐源一行，详情在 title / 再展开里），最高 40vh 滚动。
    const failureHtml = f => {
      if (!f) return ''
      if (typeof f === 'string') return `<p class="bad" role="alert">${esc(f)}</p>`
      const stage = f.stage || 'other'
      const sentence = f.hint || f.message || defaultHint(stage)
      const sources = Array.isArray(f.sources) ? f.sources : []
      const detail = f.detail || ''
      const head = `<p class="extension-failure-main" role="alert"><strong>${esc(stageText(stage))}</strong>${esc(sentence)}</p>`
      let more = ''
      if (sources.length) {
        const rows = sources.map(s => `<li><span class="extension-failure-origin">${esc(s.origin || '')}</span> · <span class="extension-failure-short">${esc(s.short || '')}</span>${s.error ? `<span class="extension-failure-raw" title="${esc(s.error)}"></span>` : ''}</li>`).join('')
        more = `<details class="extension-failure-more"><summary><span class="extension-failure-count">${esc(countText(sources.length))}</span><span class="extension-failure-toggle">${esc(text('详情', 'Details'))}</span></summary><ul class="extension-failure-sources">${rows}</ul></details>`
      } else if (detail) {
        more = `<details class="extension-failure-more"><summary><span class="extension-failure-count">${esc(text('技术细节', 'Technical details'))}</span><span class="extension-failure-toggle">${esc(text('详情', 'Details'))}</span></summary><p class="extension-failure-detail">${esc(detail)}</p></details>`
      }
      return `<div class="extension-failure" role="group" data-stage="${esc(stage)}">${head}${more}</div>`
    }
    const unlocked = id => !official(id) || (view?.star?.authenticated === true && view?.star?.verified === true)
    let authTimer, deviceCodeCopied = false, authGeneration = 0, authRetryAt = 0, authNotice = '', maintenancePaused = false
    function scheduleAuth() {
      clearTimeout(authTimer)
      const pending = view?.star?.pending
      if (maintenancePaused) return
      if (!pending) {
        const star = view?.star
        if (!star?.authenticated || star.configured === false) return
        const now = Date.now(), due = star.needsRefresh || star.needsStarCheck
        const deadlines = []
        if (Number(star.expiresAt) > 0 && !star.requiresLogin) deadlines.push(Number(star.expiresAt) - (star.refreshable ? 300000 : 0))
        const checkedAt = Number(star.lastCheckedAt || star.checkedAt)
        if (checkedAt > 0) deadlines.push(checkedAt + 86400000)
        if (!due && !deadlines.length) return
        let delay = due ? 1000 : Math.max(1000, Math.min(...deadlines) - now)
        if (authRetryAt > now) delay = Math.max(delay, authRetryAt - now)
        authTimer = setTimeout(async () => {
          if (busy) return scheduleAuth()
          await auth('check', true)
        }, Math.min(delay, 2147483647))
        return
      }
      const remaining = Number(pending.expiresAt) - Date.now()
      if (!Number.isFinite(remaining)) return
      if (remaining <= 0) { void auth('cancel'); return }
      authTimer = setTimeout(async () => {
        if (busy) return scheduleAuth()
        await auth('check', true)
      }, Math.min(remaining, Math.max(5, Number(pending.interval) || 5) * 1000))
    }
    const acceptView = next => { view = { ...next, star: next.star || view?.star }; node = view.node || ''; scheduleAuth() }
    const run = async (name, job, kind = '') => {
      if (busy) return
      busy = name; busyKind = kind; error = ''; progress = []; progressSeen.clear(); uninstallTicks = 0; actionStarted = false; redraw()
      // 安装/更新会下载：轮询宿主的进度快照（extension_progress，按 id 各一条），
      // 把「请稍候」换成带阶段/百分比/取消的进度弹窗（t29）。其它操作轮询到不活跃就停。
      const poll = () => invoke('extension_progress').then(got => {
        progress = Array.isArray(got?.items) ? got.items.filter(p => p && p.active) : []
        // t44：记住每个 id 的峰值（换源时宿主会从 0 重报，这里不回退）。
        for (const item of progress) progressSeen.set(item.id, mergeProgress(progressSeen.get(item.id), item))
        // 卸载没有宿主进度：本地按轮询节拍合成 0→100，驱动长条缩短与粒子消散。
        if (busyKind === 'uninstall' && uninstallTicks < 12) uninstallTicks += 1
        if (busy) { redraw(); progressTimer = setTimeout(poll, 150) }
      }).catch(() => { progress = [] })
      progressTimer = setTimeout(poll, 50)
      try { await job() } catch (e) { error = failureOf(e) }
      finally { busy = ''; busyKind = ''; clearTimeout(progressTimer); progress = []; progressSeen.clear(); uninstallTicks = 0; redraw() }
    }
    // t47 粒子动效：48 个小方块，canvas 绘制，requestAnimationFrame 推进。
    // 形态修正（t47，用户看 70% 截图后提的三点）：
    //   ① 目标列按序号均匀铺满 0→100%（slot=(i+0.5)/N），不再乘 0.86 把阈值挤到 41%；
    //   ② 只在**前沿**附近有粒子：粒子 i 在进度到达 slot 前 FLIGHT_WINDOW(25%≈12 个粒子)
    //      的窗口里从左侧飞向自己的列，抵达即并入、不再绘制（填过的地方保持干净）；
    //   ③ y 从条带外侧 ±10~14px 的偏移收敛到中线，读起来是“←汇聚→”。
    //   卸载 mode=out 镜像：长条右缘越过某列时，该列粒子从边缘向右飞、渐隐，长条缩短。
    // 关闭弹窗时 stopFx() 取消 rAF，且循环发现没有 canvas 也会自行结束。
    const FX_COUNT = 48
    const reducedMotion = () => typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const fxSeed = (i, salt) => { const x = Math.sin(i * 12.9898 + salt * 78.233) * 43758.5453; return x - Math.floor(x) }
    function fxParts(id) {
      if (fxParticles.has(id)) return fxParticles.get(id)
      const parts = []
      for (let i = 0; i < FX_COUNT; i++) parts.push({
        slot: (i + 0.5) / FX_COUNT,
        // 条带外侧的纵向偏移（px，画布高 56，中线 28）：±10~14px。
        side: ((i % 2 === 0) ? -1 : 1) * (10 + fxSeed(i, 1) * 4),
        size: 3 + fxSeed(i, 2) * 2.5,
      })
      fxParticles.set(id, parts)
      return parts
    }
    function startFx() {
      if (typeof requestAnimationFrame !== 'function' || typeof document === 'undefined' || reducedMotion()) return
      if (fxRaf) return
      const step = () => {
        let alive = 0
        for (const cv of document.querySelectorAll('canvas[data-ext-fx]')) { alive += 1; drawFx(cv) }
        fxRaf = alive ? requestAnimationFrame(step) : 0
      }
      fxRaf = requestAnimationFrame(step)
    }
    function stopFx() { if (fxRaf && typeof cancelAnimationFrame === 'function') cancelAnimationFrame(fxRaf); fxRaf = 0 }
    function drawFx(cv) {
      const rect = cv.getBoundingClientRect ? cv.getBoundingClientRect() : { width: 0, height: 0 }
      const w = Math.max(1, Math.round(rect.width)), h = Math.max(1, Math.round(rect.height))
      const dpr = (typeof window !== 'undefined' && window.devicePixelRatio) || 1
      if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) { cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr) }
      const ctx = cv.getContext && cv.getContext('2d'); if (!ctx) return
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0); ctx.clearRect(0, 0, w, h)
      const pct = Math.min(1, Math.max(0, Number(cv.dataset.pct || 0) / 100))
      const mode = cv.dataset.mode === 'out' ? 'out' : 'in'
      const color = (typeof getComputedStyle === 'function' ? getComputedStyle(cv).getPropertyValue('--ext-fx-particle').trim() : '') || (mode === 'out' ? 'rgba(255,255,255,.8)' : 'rgba(255,255,255,.85)')
      ctx.fillStyle = color
      const parts = fxParts(cv.dataset.extFx || '')
      const centerY = h / 2
      for (let i = 0; i < parts.length; i++) {
        const pt = parts[i]
        const flight = particleFlight(pct, i, parts.length, mode)
        if (!flight) continue   // 未出发 / 已并入 / 已消散：都不画（填过的地方必须干净）
        const q = flight.q
        const ease = 1 - (1 - q) * (1 - q) * (1 - q)
        const alpha = Math.sin(Math.PI * q)   // 起飞淡入、抵达淡出
        let x, y
        if (mode === 'in') {
          // 2026-11-01 用户要求：下载 = 卸载动画反向播放。
          // 卸载是「从该列剥离 → 向右飞散 → 淡出」；反向即
          // 「从右侧画布外飞来 → 落到该列 → 亮起」，alpha 同样用 sin(πq) 两端淡。
          const targetX = flight.slot * w
          const fromX = w + 48
          const t = 1 - q
          x = targetX + (fromX - targetX) * (t * t)
          y = centerY + pt.side * (0.25 + 0.75 * t)
        } else {
          // 从前沿（该列此刻正好是缩到位置的右缘）向右飞散。
          const startX = flight.slot * w
          x = startX + (w + 48 - startX) * (q * q)
          y = centerY + pt.side * (0.25 + 0.75 * q)
        }
        if (alpha <= 0.01) continue
        ctx.globalAlpha = alpha
        ctx.fillRect(Math.round(x - pt.size / 2), Math.round(y - pt.size / 2), pt.size, pt.size)
      }
      ctx.globalAlpha = 1
    }
    async function load(force = false) {
      if (loaded && !force) return
      loaded = true; maintenancePaused = false
      return run('load', async () => acceptView(await invoke('extension_view')))
    }
    async function readCatalog() {
      storeError = ''
      try { const got = await invoke('extension_catalog'); catalog = got.catalog; cached = !!got.cached; storeError = got.error || ''; return true }
      catch (e) { storeError = fail(e); return false }
    }
    async function store() { active = 'store'; return run('store', readCatalog) }
    async function checkUpdates() {
      return run('check', async () => {
        acceptView(await invoke('extension_view'))
        checked = await readCatalog()
      })
    }
    async function openSite(place) {
      if (!['repo', 'githubDevice', 'githubCopy'].includes(place)) return
      const device = place !== 'repo'
      if (device) deviceCodeCopied = false
      try {
        const result = await invoke('settings_open_site', { place, account: null })
        if (device) deviceCodeCopied = result?.codeCopied === true
        return result
      } finally {
        if (device && view?.star?.pending) deviceDialog({ userCode: view.star.pending.userCode, copied: deviceCodeCopied })
      }
    }
    async function auth(action, automatic = false) {
      if (!['login', 'check', 'logout', 'release', 'cancel'].includes(action)) return
      if (action === 'login') { maintenancePaused = false; authRetryAt = 0; authNotice = '' }
      if (['release', 'cancel', 'logout'].includes(action)) {
        if (action !== 'release') {
          ++authGeneration; clearTimeout(authTimer); deviceCodeCopied = false; authRetryAt = 0; authNotice = ''
          view = { ...view, star: { ...view?.star, pending: null } }; deviceDialog(null); redraw()
        }
        try {
          const star = await invoke('extension_auth', { action })
          if (action !== 'release') view = { ...view, star }
          await syncAuthFocus(); scheduleAuth()
        } catch (e) { error = fail(e) }
        finally { redraw() }
        return
      }
      if (busy) return
      const generation = action === 'login' ? ++authGeneration : authGeneration
      return run('auth', async () => {
        try {
          if (action === 'login') deviceCodeCopied = false
          const star = await invoke('extension_auth', { action })
          if (generation !== authGeneration) return
          view = { ...view, star }; authRetryAt = 0; authNotice = ''; await syncAuthFocus()
          if (action === 'login' && star.pending) await openSite('githubDevice')
        } catch (e) {
          authRetryAt = Date.now() + 60000
          if (automatic && !view?.star?.pending) authNotice = text('GitHub 暂时连接失败，登录资料已保留，将稍后自动重试。', 'GitHub is temporarily unavailable. Your saved sign-in is kept; retrying shortly.')
          if (!automatic) throw e
        }
        finally {
          if (generation === authGeneration) {
            try {
              const next = await invoke('extension_view')
              if (generation === authGeneration) {
                acceptView(next)
                if (!view.star?.authenticated || (!view.star?.needsRefresh && !view.star?.needsStarCheck)) authNotice = ''
                if (!view.star?.pending) { deviceCodeCopied = false; deviceDialog(null) }
              }
            } catch (e) {
              authRetryAt = Date.now() + 60000
              scheduleAuth()
              if (!automatic) throw e
            }
          }
        }
      })
    }
    async function action(action, id) {
      if (busy || !view || !['install', 'enabled', 'uninstall', 'autostart'].includes(action)) return
      const row = view.installed.find(r => r.plugin.manifest.id === id)
      const entry = catalog?.plugins.find(r => r.id === id)
      if (action === 'install' ? !entry : !row) return
      if (action === 'install' && !unlocked(id)) {
        error = text('请先完成 GitHub 登录与主仓 Star 验证，再安装官方插件。', 'Sign in to GitHub and verify your star before installing official extensions.')
        redraw(); return
      }
      return run(id, async () => {
        // 2026-11-01：安装直接开始，不再弹确认（用户要求）；卸载仍然确认。
        // if (action === 'install' && !(await confirm(text(`安装 ${entry.name} ${entry.version}？\n依赖：${entry.dependencies.join('、') || '无'}\n插件为独立进程，可使用声明的网络、服务及本地存储能力。`, `Install ${entry.name} ${entry.version}?\nDependencies: ${entry.dependencies.join(', ') || 'None'}\nThis extension runs as a separate local process.`)))) return
        if (action === 'uninstall' && !(await confirm(text(`卸载 ${row.plugin.manifest.name}？将停止插件并保留凭据和配置。`, `Uninstall ${row.plugin.manifest.name}? Credentials and settings will be kept.`)))) return
        actionStarted = true   // 确认都过了，从这一刻起才允许出现进度
        acceptView(await invoke('extension_manage', { action, id, enabled: action === 'install' ? true : action === 'enabled' ? !row.running : action === 'autostart' ? !autostartOn(row) : null, nodePath: null, confirm: true }))
      }, action)
    }
    async function runtime(value) {
      return run('runtime', async () => acceptView(await invoke('extension_manage', { action: 'runtime', id: null, enabled: null, nodePath: value.trim(), confirm: true })))
    }
    // 第三方插件：按 npm 包名检索与安装。来源是公开 registry，Dock 不审核其行为。
    async function thirdparty(query = thirdQuery) {
      thirdQuery = query
      active = 'thirdparty'
      const text_ = query.trim()
      if (!text_) { thirdNote = text('请输入 npm 包名或关键字', 'Type an npm package name or keyword'); redraw(); return }
      return run('third', async () => {
        const report = await invoke('extension_thirdparty', { action: 'search', query: text_, confirm: false })
        hits = report?.hits || []
        const hidden = Number(report?.hidden || 0)
        thirdSearched = true
        // 不合规包被过滤时把数量说出来，绝不静默变空列表（用户 2026-10-31）。
        thirdNote = hidden > 0
          ? text(`已隐藏 ${hidden} 个不符合 fish-dock-plugin-* 命名的包`, `Hidden ${hidden} package(s) that do not match fish-dock-plugin-*`)
          : (hits.length ? '' : text('没有找到可安装的包', 'No installable package found'))
      })
    }
    async function installThird(spec) {
      if (busy || !spec) return
      if (!await confirm(text(`安装第三方插件 ${spec}？\n它会以插件身份在本机运行代码，Dock 不审核它的行为，请只装你信任的包。`, `Install third-party extension ${spec}?\nIt runs code on this machine as a plugin; Dock does not review it. Only install packages you trust.`))) return
      return run('third', async () => {
        acceptView(await invoke('extension_thirdparty', { action: 'install', query: spec, confirm: true }))
        hits = []; thirdSearched = false; thirdNote = text('已安装：', 'Installed: ') + spec
        active = 'installed'
      })
    }
    function render() {
      const disabled = busy ? ' disabled' : ''
      const star = view?.star || {}, installed = view?.installed || [], entries = catalog?.plugins || []
      const authButton = (action, label) => {
        const unavailable = star.configured === false && (action === 'login' || (action === 'check' && !star.authenticated && !star.pending))
        return `<button type="button" class="pbtn sm" data-extension-auth="${action}"${disabled || (unavailable ? ' disabled' : '')}>${esc(label)}</button>`
      }
      const button = (action, id, label) => `<button type="button" class="pbtn sm" data-extension-action="${action}" data-extension-id="${esc(id)}"${disabled}>${esc(label)}</button>`
      const authStatus = star.requiresLogin
        ? text(`GitHub：${star.login || ''} · 登录授权需要重新登录；旧版账号首次升级需重新授权一次。`, `GitHub: ${star.login || ''} · Sign in again to restore renewable credentials.`)
        : star.authenticated
          ? star.needsStarCheck
            ? text(`GitHub：${star.login || ''} · 已登录，主仓 Star 待复核，正在自动检查。`, `GitHub: ${star.login || ''} · Signed in; automatically rechecking your repository star.`)
            : star.verified
              ? text(`GitHub：${star.login || ''} · 主仓 Star 已验证`, `GitHub: ${star.login || ''} · Repository star verified`)
              : text(`GitHub：${star.login || ''} · 已登录，请点亮主仓 Star 后检查验证。`, `GitHub: ${star.login || ''} · Signed in; star the repository, then verify.`)
          : star.configured === false
            ? text('GitHub 登录暂未开放；浏览商店、查看已安装和卸载仍可使用。', 'GitHub sign-in is not available yet. Browsing, installed status and uninstall remain available.')
            : text('登录 GitHub 并验证主仓 Star，即可安装和使用官方插件。', 'Sign in to GitHub and verify your repository star to use official extensions.')
      const starNotice = view ? `<div class="extension-auth"><p class="extension-auth-status" role="status">${esc(authStatus)}</p>${authNotice ? `<p class="note" role="status">${esc(authNotice)}</p>` : ''}<div class="extension-actions"><button type="button" class="pbtn sm extension-star" data-extension-site="repo">${esc(text('⭐ 去 Star', '⭐ Star on GitHub'))}</button>${authButton('login', text('GitHub 登录', 'Sign in to GitHub'))}${authButton('check', text('检查登录与 Star', 'Verify sign-in and star'))}${star.pending ? authButton('release', text('解除临时锁定与置顶', 'Release temporary lock and pin')) + authButton('cancel', text('取消验证', 'Cancel verification')) : ''}${star.authenticated || star.pending ? authButton('logout', text('退出登录', 'Sign out')) : ''}</div>${star.pending ? `<p class="extension-auth-code">${esc(deviceCodeCopied ? text('验证码已自动复制，在 GitHub 粘贴即可：', 'Code copied. Paste it on GitHub: ') : text('在 GitHub 输入验证码：', 'Enter this code on GitHub: '))}<strong class="mono">${esc(star.pending.userCode)}</strong> <button type="button" class="pbtn sm" data-extension-site="githubDevice">${esc(text('打开设备登录', 'Open device sign-in'))}</button></p>` : ''}</div>` : ''
      const cards = installed.map(r => {
        const m = r.plugin.manifest, update = entries.find(p => p.id === m.id && outdated(p, r.plugin))
        // 安装没走完留在盘上的那一版（plugin_install.rs: pending）：注册表里没有它，
        // 所以它既不能启用也不能卸载，只能重试安装。不显示就等于「装完了，列表里
        // 什么都没有」，连一句话都没有。
        const pending = r.plugin.pending === true
        // 两种提示别混：notice.cleanup=true 是「这一版装上了，只是旧文件没清干净」
        // （#t8 的清理警告），写成「安装未完成」就是骗人；其余才是安装真的没走完。
        const notice = r.notice
          ? (r.notice.cleanup
            ? `<p class="note" role="status">${esc(text(`v${r.notice.version} 已安装，但旧版本文件没清干净：${r.notice.reason}`, `v${r.notice.version} is installed, but old files were not fully removed: ${r.notice.reason}`))}</p>`
            : `<p class="extension-card-error bad" role="alert">${esc(text(`安装 v${r.notice.version} 未完成：${r.notice.reason}`, `Installing v${r.notice.version} did not finish: ${r.notice.reason}`))}${r.notice.hint ? `<br><span class="note">${esc(r.notice.hint)}</span>` : ''}</p>`)
          : ''
        // 这一版是从哪条源装上的（plugin_store.rs: FetchRecord）：npm 官方源 →
        // npm 镜像 → GitHub 发行包，带耗时。用户要求 UI 上能看到当前用的是哪条源；
        // 没有记录（旧安装）时这一行不出现，不占位、不写「未知」。
        const fetched = view?.fetch?.[m.id]
        const originName = fetched
          ? ({ npmRegistry: text('npm 官方源', 'npm registry'), npmMirror: text('npm 镜像', 'npm mirror'), githubRelease: text('GitHub 发行包', 'GitHub release') })[fetched.origin]
          : ''
        const origin = originName
          ? `<p class="extension-source" role="status">${esc(text('安装来源：', 'Installed from: ') + originName + ' · ' + (Number(fetched.elapsedMs || 0) / 1000).toFixed(1) + 's')}</p>`
          : ''
        if (pending) {
          const retry = unlocked(m.id) ? button('install', m.id, text('重试安装', 'Retry install')) : `<span class="note">${esc(text('登录并验证 Star 后解锁', 'Sign in and verify your star to unlock'))}</span>`
          return `<article class="extension-card"><header class="extension-card-header"><div class="extension-card-title"><strong>${esc(m.name)}</strong><span class="extension-version">${esc(m.version)}</span><span class="extension-state">${esc(text('安装未完成', 'Install did not finish'))}</span></div><div class="extension-card-actions">${retry}</div></header><p class="extension-location mono">${esc(m.storage.directory)}</p>${r.error ? `<p class="extension-card-error bad" role="alert">${esc(r.error)}</p>` : ''}${notice}${origin}</article>`
        }
        const state = r.running ? text('运行中', 'Running') : r.error ? text('启动失败', 'Failed') : r.plugin.enabled ? text('等待启动', 'Waiting to start') : text('已停用', 'Disabled')
        // 自启动钩子固定在「停用/启用」左侧（#22 / 2026-10-31 三态统一）：
        // 三态都是同一个按钮（同一个 class、同高同内衬，只有配色与文案不同）——
        // 有服务且开着＝红「关闭自启动」（点一下关掉）、有服务但关着＝蓝「随 Dock 启动」
        // （点一下打开）、没有本机服务的＝绿「无需自启动」（纯装饰：aria-disabled，
        // 不带 data-extension-action，点了不发任何 IPC）。颜色走 settings.css 的
        // --hook-on / --hook-off / --hook-none 三个主题变量，尺寸走 .pbtn.sm。
        const hookState = !m.behaviors?.autostart ? 'none' : autostartOn(r) ? 'on' : 'off'
        const hookLabel = { on: text('关闭自启动', 'Turn off autostart'), off: text('随 Dock 启动', 'Start with dock'), none: text('无需自启动', 'No autostart') }[hookState]
        const hookTitle = {
          on: text('点一下关掉：Dock 启动时不再拉起这个插件的本机服务', 'Click to turn off: the Dock will not start this extension’s service'),
          off: text('点一下打开：Dock 启动时这个插件自己把本机服务拉起来', 'Click to turn on: the extension starts its service with the Dock'),
          none: text('这个插件没有本机服务，不需要随 Dock 启动', 'This extension has no local service to start with the Dock'),
        }[hookState]
        const hookAction = hookState === 'none'
          ? ' aria-disabled="true"'
          : ` data-extension-action="autostart" data-extension-id="${esc(m.id)}" aria-pressed="${hookState === 'on'}"`
        const hook = `<button type="button" class="pbtn sm hook" data-hook="${hookState}"${hookAction}${disabled} title="${esc(hookTitle)}">${esc(hookLabel)}</button>`
        const actions = `${update && unlocked(m.id) ? button('install', m.id, text('更新', 'Update')) : ''}${hook}${r.running || unlocked(m.id) ? button('enabled', m.id, r.running ? text('停用', 'Disable') : r.error ? text('重试启用', 'Retry') : text('启用', 'Enable')) : ''}${button('uninstall', m.id, text('卸载', 'Uninstall'))}`
        const pages = unlocked(m.id) ? m.pages.map(p => `<button type="button" class="pbtn sm" data-extension-page="${esc(m.id + '/' + p.id)}"${r.running ? disabled : ' disabled'}>${esc(p.title)}</button>`).join('') : `<span class="note">${esc(text('主仓 Star 验证后解锁功能页', 'Verify your repository star to unlock pages'))}</span>`
        return `<article class="extension-card"><header class="extension-card-header"><div class="extension-card-title"><strong>${esc(m.name)}</strong><span class="extension-version">${esc(m.version)}</span><span class="extension-state">${esc(state)}</span>${update ? `<span class="extension-update">${esc(text('有更新 v', 'Update available v') + update.version)}</span>` : ''}</div><div class="extension-card-actions">${actions}</div></header><p class="extension-location mono">${esc(m.storage.directory)}</p>${origin}${r.error ? `<p class="extension-card-error bad" role="alert">${esc(r.error)}</p>` : ''}${notice}${m.pages.length ? `<div class="extension-pages">${pages}</div>` : ''}</article>`
      }).join('')
      const storeCards = entries.map(p => {
        const existing = installed.find(r => r.plugin.manifest.id === p.id && r.plugin.pending !== true), update = existing && outdated(p, existing.plugin)
        const actions = !unlocked(p.id) ? `<span class="note">${esc(text('登录并验证 Star 后解锁', 'Sign in and verify your star to unlock'))}</span>` : existing && !update && !existing.packageMissing ? `<span class="d">${esc(text('已安装', 'Installed'))}</span>` : button('install', p.id, existing?.packageMissing ? text('重新安装', 'Reinstall') : existing ? text('更新', 'Update') : text('安装', 'Install'))
        // 作者按数组渲染（「A、B」）；维护者字段 2.0.0 起整体去掉，卡上不再有这一行
        //（#22-4）。旧目录已被宿主归一成同一结构，这里不再读 author/maintainer。
        const authors = esc(authorsOf(p).join(text('、', ', ')) || '—')
        // 目录条目可带一行说明（作者/维护状态）。
        const note = p.note ? `<p class="extension-note">${esc(p.note)}</p>` : ''
        return `<article class="extension-card"><header class="extension-card-header"><div class="extension-card-title"><strong>${esc(p.name)}</strong><span class="extension-version">${esc(p.version)}</span>${update ? `<span class="extension-update">${esc(text('有更新 v', 'Update available v') + p.version)}</span>` : ''}</div><div class="extension-card-actions">${actions}</div></header><p class="extension-description">${esc(p.description)}</p>${note}<dl class="extension-meta"><div><dt>${esc(text('作者', 'Author'))}</dt><dd>${authors}</dd></div><div><dt>API</dt><dd>${p.hostApi}</dd></div><div><dt>${esc(text('平台', 'Platform'))}</dt><dd>${esc(p.platform)}</dd></div><div><dt>${esc(text('大小', 'Size'))}</dt><dd>${size(p.bytes)}</dd></div></dl><p class="extension-dependencies"><span>${esc(text('依赖', 'Dependencies'))}</span>${esc(p.dependencies.join('、') || text('无', 'None'))}</p></article>`
      }).join('')
      const thirdPanel = `<div class="extension-third"><div class="extension-third-search"><input id="extension-third-query" type="text" placeholder="${esc(text('包名需为 fish-dock-plugin-<名字>，如 @acme/fish-dock-plugin-weather', 'Package must be fish-dock-plugin-<name>, e.g. @acme/fish-dock-plugin-weather'))}" value="${esc(thirdQuery)}" autocomplete="off" spellcheck="false"${disabled}><button type="button" class="pbtn" data-extension-third="search"${disabled}>${esc(text('检索', 'Search'))}</button></div>${thirdNote ? `<p role="status" class="note">${esc(thirdNote)}</p>` : ''}<p class="note">${esc(text('第三方插件来自公开 npm registry，由发布者维护；Dock 只校验包结构与摘要，不审核其行为。', 'Third-party extensions come from the public npm registry and are maintained by their publishers; Dock only checks structure and digests.'))}</p>${hits.map(hit => `<article class="extension-card"><header class="extension-card-header"><div class="extension-card-title"><strong>${esc(hit.name)}</strong><span class="extension-version">${esc(hit.version)}</span></div><div class="extension-card-actions"><button type="button" class="pbtn sm" data-extension-third="install" data-third="${esc(hit.name + '@' + hit.version)}"${disabled}>${esc(text('安装', 'Install'))}</button></div></header><p class="extension-description">${esc(hit.description || '')}</p><dl class="extension-meta"><div><dt>${esc(text('发布者', 'Publisher'))}</dt><dd>${esc(hit.publisher || '—')}</dd></div><div><dt>${esc(text('更新', 'Updated'))}</dt><dd>${esc(String(hit.updated || '').slice(0, 10) || '—')}</dd></div></dl></article>`).join('')}${thirdSearched && !hits.length && !thirdNote ? `<p class="note">${esc(text('可换一个包名再试。', 'Try another package name.'))}</p>` : ''}</div>`
      const runtime = view?.runtime
      const sourceLabel = source => ({ saved: text('手动配置', 'Manual override'), bundled: text('随包运行时', 'Bundled runtime'), path: text('系统 PATH', 'System PATH'), development: text('开发环境覆盖', 'Development override') })[source] || text('尚未找到', 'Not found')
      const runtimeStatus = runtime ? `<p class="note">${esc(runtime.installed ? text('正式发行构建', 'Installed release') : text('开发构建', 'Development build'))} · ${esc(sourceLabel(runtime.source))} ${esc(runtime.version || '')}</p>${runtime.path ? `<p class="extension-location mono">${esc(runtime.path)}</p>` : ''}${runtime.warning ? `<p role="status" class="note">${esc(runtime.warning)}</p>` : ''}` : ''
      const updates = installed.filter(r => r.plugin.pending !== true && entries.some(p => p.id === r.plugin.manifest.id && outdated(p, r.plugin))).length
      const pendingCount = installed.filter(r => r.plugin.pending === true).length
      const result = checked ? `<p role="status" class="note">${esc((cached ? text('按缓存目录：', 'From cached catalog: ') : '') + (updates ? text(`${updates} 个插件有更新`, `${updates} extension updates available`) : text('已安装插件已是最新版本', 'Installed extensions are up to date')))}${pendingCount ? esc(text(`；另有 ${pendingCount} 个安装未完成`, `; ${pendingCount} install(s) did not finish`)) : ''}</p>` : ''
      // t44 进度弹窗：插件名 + 阶段（动词+中）+ 百分比 + 字节（1024 自动进位）+ 取消；
      // 条形上还叠一层粒子 canvas（安装从左飞入汇聚、卸载向右剥离消散）。
      const phaseName = phase => { const word = PHASE_WORDS[phase]; return word ? text(word[0], word[1]) : (phase || text('下载中', 'Downloading')) }
      const progressMetaOf = p => installed.map(r => r.plugin.manifest).find(m => m.id === p.id) || entries.find(entry => entry.id === p.id)
      const progressItems = () => {
        const items = progress.slice()
        // 卸载没有宿主进度：面板本地合成一张卡（phase 卸载，0→96%），节拍来自同一轮询。
        if (busyKind === 'uninstall' && busy && actionStarted && !items.some(item => item.id === busy)) {
          items.push({ id: busy, phase: '卸载', received: Math.min(96, uninstallTicks * 8), total: 100, active: true, local: 'out' })
        }
        return items
      }
      const progressCard = p => {
        const meta = progressMetaOf(p)
        const name = (meta && meta.name) || p.id || ''
        const seen = progressSeen.get(p.id) || mergeProgress(null, p)
        const known = Number(p.total) > 0 || seen.raw != null
        const pct = known ? seen.peak : null
        const out = p.local === 'out' || p.phase === '卸载'
        const width = pct == null ? 30 : (out ? Math.max(2, 100 - pct) : pct)
        const phaseText = known && seen.switching && !out ? text('切换来源', 'Switching source') : phaseName(p.phase)
        const amount = p.total ? `${formatBytes(p.received)} / ${formatBytes(p.total)}` : formatBytes(p.received)
        return `<div class="ext-progress"><div class="ph"><strong>${esc(name)}</strong><span>${esc(phaseText)}</span></div><div class="meter"><div class="bar${pct == null ? ' unsure' : ''}" role="progressbar" aria-valuemin="0" aria-valuemax="100"${pct == null ? '' : ` aria-valuenow="${pct}"`}><i style="width:${width}%"></i></div></div><div class="detail">${p.local === 'out' ? `${pct == null ? 0 : pct}%` : `${pct == null ? esc(text('总量未知', 'Size unknown')) : `${pct}%`} · ${esc(amount)}`}</div><div class="foot"><button type="button" class="pbtn sm" data-extension-cancel="${esc(p.id || '')}">${esc(text('取消', 'Cancel'))}</button></div></div>`
      }
      const items = progressItems()
      const progressHTML = items.length ? `<div class="ext-progress-backdrop" role="dialog" aria-modal="true" aria-label="${esc(text('插件下载进度', 'Extension download progress'))}">${items.map(progressCard).join('')}</div>` : ''
      return `<section class="extension-panel" aria-labelledby="extension-heading" aria-busy="${!!busy}"><h3 id="extension-heading">${esc(text('可安装扩展', 'Installable extensions'))}</h3>${starNotice}<div class="extension-actions"><button type="button" class="pbtn" data-extension-view="installed" aria-pressed="${active === 'installed'}"${disabled}>${esc(text('已安装', 'Installed'))}</button><button type="button" class="pbtn" data-extension-view="store" aria-pressed="${active === 'store'}"${disabled}>${esc(text('官方插件', 'Official plugins'))}</button><button type="button" class="pbtn" data-extension-view="thirdparty" aria-pressed="${active === 'thirdparty'}"${disabled}>${esc(text('第三方插件', 'Third-party plugins'))}</button><button type="button" class="pbtn" data-extension-refresh${disabled}>${esc(text('检查更新', 'Check for updates'))}</button></div>${result}${storeError ? `<p class="bad" role="alert">${esc(storeError)}</p>` : ''}<div class="extension-list">${active === 'thirdparty' ? thirdPanel : active === 'installed' ? cards || `<p class="note">${esc(text('尚未安装扩展。可从官方商店按需选择，现有小工具仍在下方。', 'No extensions installed. Choose from the official store. Existing widgets remain below.'))}</p>` : `${cached ? `<p role="status">${esc(text('当前使用缓存目录', 'Using cached catalog'))}</p>` : ''}${storeCards || `<p class="note">${esc(busy === 'store' ? text('正在读取官方目录…', 'Loading official catalog…') : text('暂无可用目录，请检查更新重试。', 'No catalog available. Check for updates to retry.'))}</p>`}${!entries.length && cards ? `<h4>${esc(text('已安装扩展', 'Installed extensions'))}</h4>${cards}` : ''}`}</div><details><summary>${esc(text('运行依赖', 'Runtime dependencies'))}</summary>${runtimeStatus}<label for="extension-node">${esc(text('Node 可执行文件路径', 'Node executable path'))}</label><input id="extension-node" class="extension-node" type="text" value="${esc(node)}" spellcheck="false"${disabled}><button type="button" class="pbtn" data-extension-runtime${disabled}>${esc(text('保存依赖路径', 'Save runtime path'))}</button><p class="note">${esc(text('Node 插件使用已校验的宿主 Node。开发环境可指定完整路径；留空恢复宿主运行资源。', 'Extensions use the verified host Node. Development builds can specify a full path. Leave empty to restore the host runtime.'))}</p></details>${progressHTML}${busy && !progressHTML ? `<p role="status">${esc(text('正在执行，请稍候…', 'Working, please wait…'))}</p>` : ''}${failureHtml(error)}</section>`
    }
    function mount(root) {
      const panel = root.querySelector('.extension-panel'); if (!panel) return
      panel.querySelectorAll('[data-extension-view]').forEach(b => { b.onclick = () => { if (busy) return; const want = b.dataset.extensionView; if (want === 'store') void store(); else if (want === 'thirdparty') { active = 'thirdparty'; redraw() } else { active = 'installed'; redraw() } } })
      panel.querySelectorAll('[data-extension-site]').forEach(b => { b.onclick = () => run('open', () => openSite(b.dataset.extensionSite)) })
      panel.querySelectorAll('[data-extension-auth]').forEach(b => { b.onclick = () => auth(b.dataset.extensionAuth) })
      panel.querySelector('[data-extension-refresh]').onclick = () => checkUpdates()
      panel.querySelectorAll('[data-extension-action]').forEach(b => { b.onclick = () => action(b.dataset.extensionAction, b.dataset.extensionId) })
      // t29：进度弹窗的取消——真取消（宿主在下一个块返回「已取消」）。
      panel.querySelectorAll('[data-extension-cancel]').forEach(b => { b.onclick = () => { invoke('extension_cancel', { id: b.dataset.extensionCancel }); } })
      panel.querySelectorAll('[data-extension-page]').forEach(b => { b.onclick = () => { const id = b.dataset.extensionPage.split('/')[0]; if (unlocked(id)) openPage(b.dataset.extensionPage) } })
      panel.querySelector('[data-extension-runtime]').onclick = () => runtime(panel.querySelector('#extension-node').value)
      const thirdBox = panel.querySelector('#extension-third-query')
      if (thirdBox) {
        thirdBox.oninput = () => { thirdQuery = thirdBox.value }
        thirdBox.onkeydown = e => { if (e.key === 'Enter') { e.preventDefault(); void thirdparty() } }
      }
      panel.querySelectorAll('[data-extension-third]').forEach(b => { b.onclick = () => b.dataset.extensionThird === 'search' ? thirdparty() : installThird(b.dataset.third) })
      // t44：挂载时若进度弹窗已经在（例如切回本页），重新点燃粒子循环；轮询也会再点。
      // 2026-11-01 用户要求：去掉粒子动效，只保留最基本的进度条（不再启动 rAF）
      void load()
    }
    return { render, mount, load, store, action, runtime, auth, thirdparty, installThird, closeAuth: () => { maintenancePaused = true; ++authGeneration; clearTimeout(authTimer); stopFx(); return view?.star?.pending ? auth('cancel') : Promise.resolve() }, pages: () => (view?.installed || []).filter(r => r.running && unlocked(r.plugin.manifest.id)).flatMap(r => r.plugin.manifest.pages.map(p => ({ ...p, plugin: r.plugin.manifest.id, digest: r.plugin.digest, key: r.plugin.manifest.id + '/' + p.id }))) }
  }
  window.ExtensionPanel = { create, formatBytes, mergeProgress, particleFlight, PHASE_WORDS, FLIGHT_WINDOW }
})()
