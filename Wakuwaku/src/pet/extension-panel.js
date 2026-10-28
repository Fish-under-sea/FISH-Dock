// 扩展管理状态以宿主结果为准，离线目录不遮住卸载和故障处理入口。
;(function () {
  const esc = value => String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c])
  const size = bytes => {
    let value = Math.max(0, Number(bytes) || 0), unit = 0
    const units = ['B', 'KB', 'MB', 'GB']
    while (value >= 1000 && unit < units.length - 1) { value /= 1000; unit++ }
    return `${Number(value.toFixed(unit ? 1 : 0))} ${units[unit]}`
  }
  const official = id => ['fish.feishu', 'fish.clash', 'fish.magpie', 'fish.ofm'].includes(id)
  const newer = (candidate, current) => {
    const parse = value => /^\d+\.\d+\.\d+$/.test(String(value)) ? String(value).split('.').map(Number) : null
    const a = parse(candidate), b = parse(current)
    if (!a || !b) return false
    for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] > b[i]
    return false
  }
  function create({ invoke, confirm, lang, redraw, openPage = () => {}, syncAuthFocus = async () => {}, deviceDialog = () => {} }) {
    let view = null, catalog = null, cached = false, storeError = '', error = '', busy = '', loaded = false, active = 'installed', node = '', checked = false
    let hits = [], thirdQuery = '', thirdNote = '', thirdSearched = false
    const text = (zh, en) => lang() === 'en' ? en : zh
    const fail = e => typeof e === 'string' ? e : e?.message || text('插件操作失败，请重试。', 'Extension operation failed. Please retry.')
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
    const run = async (name, job) => {
      if (busy) return
      busy = name; error = ''; redraw()
      try { await job() } catch (e) { error = fail(e) }
      finally { busy = ''; redraw() }
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
      if (busy || !view || !['install', 'enabled', 'uninstall'].includes(action)) return
      const row = view.installed.find(r => r.plugin.manifest.id === id)
      const entry = catalog?.plugins.find(r => r.id === id)
      if (action === 'install' ? !entry : !row) return
      if (action === 'install' && !unlocked(id)) {
        error = text('请先完成 GitHub 登录与主仓 Star 验证，再安装官方插件。', 'Sign in to GitHub and verify your star before installing official extensions.')
        redraw(); return
      }
      return run(id, async () => {
        if (action === 'install' && !(await confirm(text(`安装 ${entry.name} ${entry.version}？\n依赖：${entry.dependencies.join('、') || '无'}\n插件为独立进程，可使用声明的网络、服务及本地存储能力。`, `Install ${entry.name} ${entry.version}?\nDependencies: ${entry.dependencies.join(', ') || 'None'}\nThis extension runs as a separate local process.`)))) return
        if (action === 'uninstall' && !(await confirm(text(`卸载 ${row.plugin.manifest.name}？将停止插件并保留凭据和配置。`, `Uninstall ${row.plugin.manifest.name}? Credentials and settings will be kept.`)))) return
        acceptView(await invoke('extension_manage', { action, id, enabled: action === 'install' ? true : action === 'enabled' ? !row.running : null, nodePath: null, confirm: true }))
      })
    }
    async function runtime(value) {
      return run('runtime', async () => acceptView(await invoke('extension_manage', { action: 'runtime', id: null, enabled: null, nodePath: value.trim(), confirm: true })))
    }
    // 第三方插件：按 npm 包名检索与安装。来源是公开 registry，Dock 不审核其行为。
    async function thirdparty(query = thirdQuery) {
      thirdQuery = query
      const text_ = query.trim()
      if (!text_) { thirdNote = text('请输入 npm 包名或关键字', 'Type an npm package name or keyword'); redraw(); return }
      return run('third', async () => {
        hits = (await invoke('extension_thirdparty', { action: 'search', query: text_, confirm: false }))?.hits || []
        thirdSearched = true
        thirdNote = hits.length ? '' : text('没有找到可安装的包', 'No installable package found')
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
        const m = r.plugin.manifest, update = entries.find(p => p.id === m.id && newer(p.version, m.version))
        const state = r.running ? text('运行中', 'Running') : r.error ? text('启动失败', 'Failed') : r.plugin.enabled ? text('等待启动', 'Waiting to start') : text('已停用', 'Disabled')
        const actions = `${update && unlocked(m.id) ? button('install', m.id, text('更新', 'Update')) : ''}${r.running || unlocked(m.id) ? button('enabled', m.id, r.running ? text('停用', 'Disable') : r.error ? text('重试启用', 'Retry') : text('启用', 'Enable')) : ''}${button('uninstall', m.id, text('卸载', 'Uninstall'))}`
        const pages = unlocked(m.id) ? m.pages.map(p => `<button type="button" class="pbtn sm" data-extension-page="${esc(m.id + '/' + p.id)}"${r.running ? disabled : ' disabled'}>${esc(p.title)}</button>`).join('') : `<span class="note">${esc(text('主仓 Star 验证后解锁功能页', 'Verify your repository star to unlock pages'))}</span>`
        return `<article class="extension-card"><header class="extension-card-header"><div class="extension-card-title"><strong>${esc(m.name)}</strong><span class="extension-version">${esc(m.version)}</span><span class="extension-state">${esc(state)}</span>${update ? `<span class="extension-update">${esc(text('有更新 v', 'Update available v') + update.version)}</span>` : ''}</div><div class="extension-card-actions">${actions}</div></header><p class="extension-location mono">${esc(m.storage.directory)}</p>${r.error ? `<p class="extension-card-error bad" role="alert">${esc(r.error)}</p>` : ''}${m.pages.length ? `<div class="extension-pages">${pages}</div>` : ''}</article>`
      }).join('')
      const storeCards = entries.map(p => {
        const existing = installed.find(r => r.plugin.manifest.id === p.id), update = existing && newer(p.version, existing.plugin.manifest.version)
        const actions = !unlocked(p.id) ? `<span class="note">${esc(text('登录并验证 Star 后解锁', 'Sign in and verify your star to unlock'))}</span>` : existing && !update && !existing.packageMissing ? `<span class="d">${esc(text('已安装', 'Installed'))}</span>` : button('install', p.id, existing?.packageMissing ? text('重新安装', 'Reinstall') : existing ? text('更新', 'Update') : text('安装', 'Install'))
        return `<article class="extension-card"><header class="extension-card-header"><div class="extension-card-title"><strong>${esc(p.name)}</strong><span class="extension-version">${esc(p.version)}</span>${update ? `<span class="extension-update">${esc(text('有更新 v', 'Update available v') + p.version)}</span>` : ''}</div><div class="extension-card-actions">${actions}</div></header><p class="extension-description">${esc(p.description)}</p><dl class="extension-meta"><div><dt>${esc(text('作者', 'Author'))}</dt><dd>${esc(p.author)}</dd></div><div><dt>${esc(text('维护者', 'Maintainer'))}</dt><dd>${esc(p.maintainer)}</dd></div><div><dt>API</dt><dd>${p.hostApi}</dd></div><div><dt>${esc(text('平台', 'Platform'))}</dt><dd>${esc(p.platform)}</dd></div><div><dt>${esc(text('大小', 'Size'))}</dt><dd>${size(p.bytes)}</dd></div></dl><p class="extension-dependencies"><span>${esc(text('依赖', 'Dependencies'))}</span>${esc(p.dependencies.join('、') || text('无', 'None'))}</p></article>`
      }).join('')
      const thirdPanel = `<div class="extension-third"><div class="extension-third-search"><input id="extension-third-query" type="text" placeholder="${esc(text('npm 包名，如 @acme/fish-weather', 'npm package, e.g. @acme/fish-weather'))}" value="${esc(thirdQuery)}" autocomplete="off" spellcheck="false"${disabled}><button type="button" class="pbtn" data-extension-third="search"${disabled}>${esc(text('检索', 'Search'))}</button></div><p class="note">${esc(text('第三方插件来自公开 npm registry，由发布者维护；Dock 只校验包结构与摘要，不审核其行为。', 'Third-party extensions come from the public npm registry and are maintained by their publishers; Dock only checks structure and digests.'))}</p>${thirdNote ? `<p role="status" class="note">${esc(thirdNote)}</p>` : ''}${hits.map(hit => `<article class="extension-card"><header class="extension-card-header"><div class="extension-card-title"><strong>${esc(hit.name)}</strong><span class="extension-version">${esc(hit.version)}</span></div><div class="extension-card-actions"><button type="button" class="pbtn sm" data-extension-third="install" data-third="${esc(hit.name + '@' + hit.version)}"${disabled}>${esc(text('安装', 'Install'))}</button></div></header><p class="extension-description">${esc(hit.description || '')}</p><dl class="extension-meta"><div><dt>${esc(text('发布者', 'Publisher'))}</dt><dd>${esc(hit.publisher || '—')}</dd></div><div><dt>${esc(text('更新', 'Updated'))}</dt><dd>${esc(String(hit.updated || '').slice(0, 10) || '—')}</dd></div></dl></article>`).join('')}${thirdSearched && !hits.length ? `<p class="note">${esc(text('可换一个包名再试。', 'Try another package name.'))}</p>` : ''}</div>`
      const runtime = view?.runtime
      const sourceLabel = source => ({ saved: text('手动配置', 'Manual override'), bundled: text('随包运行时', 'Bundled runtime'), path: text('系统 PATH', 'System PATH'), development: text('开发环境覆盖', 'Development override') })[source] || text('尚未找到', 'Not found')
      const runtimeStatus = runtime ? `<p class="note">${esc(runtime.installed ? text('正式发行构建', 'Installed release') : text('开发构建', 'Development build'))} · ${esc(sourceLabel(runtime.source))} ${esc(runtime.version || '')}</p>${runtime.path ? `<p class="extension-location mono">${esc(runtime.path)}</p>` : ''}${runtime.warning ? `<p role="status" class="note">${esc(runtime.warning)}</p>` : ''}` : ''
      const updates = installed.filter(r => entries.some(p => p.id === r.plugin.manifest.id && newer(p.version, r.plugin.manifest.version))).length
      const result = checked ? `<p role="status" class="note">${esc((cached ? text('按缓存目录：', 'From cached catalog: ') : '') + (updates ? text(`${updates} 个插件有更新`, `${updates} extension updates available`) : text('已安装插件已是最新版本', 'Installed extensions are up to date')))}</p>` : ''
      return `<section class="extension-panel" aria-labelledby="extension-heading" aria-busy="${!!busy}"><h3 id="extension-heading">${esc(text('可安装扩展', 'Installable extensions'))}</h3>${starNotice}<div class="extension-actions"><button type="button" class="pbtn" data-extension-view="installed" aria-pressed="${active === 'installed'}"${disabled}>${esc(text('已安装', 'Installed'))}</button><button type="button" class="pbtn" data-extension-view="store" aria-pressed="${active === 'store'}"${disabled}>${esc(text('官方插件', 'Official plugins'))}</button><button type="button" class="pbtn" data-extension-view="thirdparty" aria-pressed="${active === 'thirdparty'}"${disabled}>${esc(text('第三方插件', 'Third-party plugins'))}</button><button type="button" class="pbtn" data-extension-refresh${disabled}>${esc(text('检查更新', 'Check for updates'))}</button></div>${result}${storeError ? `<p class="bad" role="alert">${esc(storeError)}</p>` : ''}<div class="extension-list">${active === 'thirdparty' ? thirdPanel : active === 'installed' ? cards || `<p class="note">${esc(text('尚未安装扩展。可从官方商店按需选择，现有小工具仍在下方。', 'No extensions installed. Choose from the official store. Existing widgets remain below.'))}</p>` : `${cached ? `<p role="status">${esc(text('当前使用缓存目录', 'Using cached catalog'))}</p>` : ''}${storeCards || `<p class="note">${esc(busy === 'store' ? text('正在读取官方目录…', 'Loading official catalog…') : text('暂无可用目录，请检查更新重试。', 'No catalog available. Check for updates to retry.'))}</p>`}${!entries.length && cards ? `<h4>${esc(text('已安装扩展', 'Installed extensions'))}</h4>${cards}` : ''}`}</div><details><summary>${esc(text('运行依赖', 'Runtime dependencies'))}</summary>${runtimeStatus}<label for="extension-node">${esc(text('Node 可执行文件路径', 'Node executable path'))}</label><input id="extension-node" class="extension-node" type="text" value="${esc(node)}" spellcheck="false"${disabled}><button type="button" class="pbtn" data-extension-runtime${disabled}>${esc(text('保存依赖路径', 'Save runtime path'))}</button><p class="note">${esc(text('Node 插件使用已校验的宿主 Node。开发环境可指定完整路径；留空恢复宿主运行资源。', 'Extensions use the verified host Node. Development builds can specify a full path. Leave empty to restore the host runtime.'))}</p></details>${busy ? `<p role="status">${esc(text('正在执行，请稍候…', 'Working, please wait…'))}</p>` : ''}${error ? `<p class="bad" role="alert">${esc(error)}</p>` : ''}</section>`
    }
    function mount(root) {
      const panel = root.querySelector('.extension-panel'); if (!panel) return
      panel.querySelectorAll('[data-extension-view]').forEach(b => { b.onclick = () => { if (busy) return; const want = b.dataset.extensionView; if (want === 'store') void store(); else if (want === 'thirdparty') { active = 'thirdparty'; redraw() } else { active = 'installed'; redraw() } } })
      panel.querySelectorAll('[data-extension-site]').forEach(b => { b.onclick = () => run('open', () => openSite(b.dataset.extensionSite)) })
      panel.querySelectorAll('[data-extension-auth]').forEach(b => { b.onclick = () => auth(b.dataset.extensionAuth) })
      panel.querySelector('[data-extension-refresh]').onclick = () => checkUpdates()
      panel.querySelectorAll('[data-extension-action]').forEach(b => { b.onclick = () => action(b.dataset.extensionAction, b.dataset.extensionId) })
      panel.querySelectorAll('[data-extension-page]').forEach(b => { b.onclick = () => { const id = b.dataset.extensionPage.split('/')[0]; if (unlocked(id)) openPage(b.dataset.extensionPage) } })
      panel.querySelector('[data-extension-runtime]').onclick = () => runtime(panel.querySelector('#extension-node').value)
      const thirdBox = panel.querySelector('#extension-third-query')
      if (thirdBox) {
        thirdBox.oninput = () => { thirdQuery = thirdBox.value }
        thirdBox.onkeydown = e => { if (e.key === 'Enter') { e.preventDefault(); void thirdparty() } }
      }
      panel.querySelectorAll('[data-extension-third]').forEach(b => { b.onclick = () => b.dataset.extensionThird === 'search' ? thirdparty() : installThird(b.dataset.third) })
      void load()
    }
    return { render, mount, load, store, action, runtime, auth, thirdparty, installThird, closeAuth: () => { maintenancePaused = true; ++authGeneration; clearTimeout(authTimer); return view?.star?.pending ? auth('cancel') : Promise.resolve() }, pages: () => (view?.installed || []).filter(r => r.running && unlocked(r.plugin.manifest.id)).flatMap(r => r.plugin.manifest.pages.map(p => ({ ...p, plugin: r.plugin.manifest.id, digest: r.plugin.digest, key: r.plugin.manifest.id + '/' + p.id }))) }
  }
  window.ExtensionPanel = { create }
})()
