// The Clash page, drawn by the dock itself: the network cards (system proxy,
// subscription) up top, the proxy groups and their nodes as a card grid below
// (Clash Verge Rev's shape). The kernel's controller API is the data source,
// reached through clash_api (clash.rs) so the controller's secret never enters
// this page. The zashboard console remains reachable as the advanced panel,
// opened in the person's browser rather than in a frame.
(function () {
  const invoke = (command, args) => window.__TAURI__?.core?.invoke?.(command, args)

  let T = key => key
  let el = null
  // 全量数据与选择：组、节点、当前组、动作条状态。
  let proxies = null
  let selGroup = ''
  let sysproxy = false
  let info = { version: '', mixed: 0, controller: 0 }
  let subValue = ''
  let busy = ''
  let errorText = ''

  const esc = value =>
    String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c])

  // RFC 3986 严格编码：节点名（中文、emoji、|）进路径段之前。
  const enc = value => encodeURIComponent(String(value)).replace(/[!'()*]/g, c => `%${c.charCodeAt(0).toString(16).toUpperCase()}`)

  async function api(method, path, body) {
    return invoke('clash_api', { method, path, body })
  }

  // 组：有 all 字段的都是组（Selector/URLTest/Fallback/…）；GLOBAL 排到最后。
  function groups() {
    const all = Object.values(proxies || {}).filter(p => Array.isArray(p.all))
    all.sort((a, b) => (a.name === 'GLOBAL') - (b.name === 'GLOBAL'))
    return all
  }

  // 延迟：history 的最后一个非零值；0 = 测过但失败（Error）。
  function delayOf(node) {
    const history = node?.history || []
    for (let at = history.length - 1; at >= 0; at--) {
      const value = Number(history[at]?.delay ?? 0)
      if (value > 0) return value
      if (value === 0 && history[at]?.delay === 0) return 0
    }
    return null
  }

  const delayClass = value =>
    value == null ? 'none' : value === 0 ? 'bad' : value < 200 ? 'good' : value < 500 ? 'mid' : 'slow'

  function nodeCard(name, node, current) {
    const delay = delayOf(node)
    const badges = [node.type, node.udp ? 'UDP' : ''].filter(Boolean).map(b => `<span class="cp-badge">${esc(b)}</span>`).join('')
    const delayWords = delay == null ? '—' : delay === 0 ? 'Error' : String(delay)
    return `<button class="cp-node${current ? ' current' : ''}" data-node="${esc(name)}" data-group="${esc(selGroup)}"><span class="cp-node-name" title="${esc(name)}">${esc(name)}</span><span class="cp-delay ${delayClass(delay)}">${delayWords}</span><span class="cp-badges">${badges}</span></button>`
  }

  function groupRow(group, active) {
    return `<button class="cp-group${active ? ' active' : ''}" data-group-tab="${esc(group.name)}"><span class="cp-group-name" title="${esc(group.name)}">${esc(group.name)}</span><span class="cp-group-kind">${esc(group.type)} · ${group.all.length}</span></button>`
  }

  function render() {
    if (!el) return
    const list = groups()
    if (!selGroup && list.length) selGroup = list[0].name
    const group = list.find(g => g.name === selGroup)
    const nodes = group?.all || []
    const cards = nodes.map(name => nodeCard(name, proxies?.[name] || { name }, group?.now === name)).join('')
    const boxes = [
      {
        icon: '🌐',
        title: T('s.clashNetTitle'),
        body:
          `<div class="cp-rows"><div class="cp-row${sysproxy ? ' on' : ''}"><span class="cp-row-icon">⇄</span><span class="cp-row-name">${esc(T('s.clashSysproxy'))}</span><label class="cp-switch"><input type="checkbox" data-clash-sysproxy ${sysproxy ? 'checked' : ''}><span class="cp-knob"></span></label></div>` +
          `<div class="cp-hint ${sysproxy ? 'ok' : ''}">${esc(sysproxy ? T('s.clashProxyOn') : T('s.clashProxyOff'))}</div></div>`,
      },
      {
        icon: '📡',
        title: T('s.clashSubTitle'),
        body:
          `<div class="cp-rows"><div class="cp-row plain"><input class="cp-sub" type="url" placeholder="${esc(T('s.clashSubHint'))}" value="${esc(subValue)}"><button class="pbtn" data-clash-sub-save>${esc(T('s.clashSubSave'))}</button><button class="pbtn" data-clash-sub-update ${subValue ? '' : 'disabled'}>${esc(T('s.clashSubUpdate'))}</button></div>` +
          `<div class="cp-hint">${esc(T('s.clashSubNote'))}</div></div>`,
      },
    ]
      .map(card => `<div class="cp-card"><span class="cp-card-icon">${card.icon}</span><span class="cp-card-title">${esc(card.title)}</span>${card.body}</div>`)
      .join('')

    el.innerHTML =
      `<div class="cp-top">${boxes}<div class="cp-meta">${esc(info.version || '')}${info.mixed ? ` · ${esc(T('s.clashPort'))} ${info.mixed}` : ''}<button class="pbtn" data-clash-advanced>${esc(T('s.clashAdvanced'))} ↗</button><button class="pbtn" data-clash-toggle>${esc(info.version ? T('s.clashStop') : T('s.clashStart'))}</button></div></div>` +
      `<div class="cp-main"><div class="cp-groups">${busy === 'testing' ? `<div class="cp-testing">${esc(T('s.clashTesting'))}</div>` : ''}${list.map(g => groupRow(g, g.name === selGroup)).join('')}</div>` +
      `<div class="cp-nodes"><div class="cp-head"><span class="cp-head-name">${esc(group?.name || '')}</span><span class="cp-head-kind">${esc(group?.type || '')} · ${nodes.length}</span><span class="cp-head-now">${group?.now ? `→ ${esc(group.now)}` : ''}</span></div>` +
      `<div class="cp-grid">${cards || `<div class="cp-empty">${esc(T('s.clashNoNodes'))}</div>`}</div></div></div>` +
      (errorText ? `<div class="cp-error">${esc(errorText)}</div>` : '')
  }

  async function refresh({ keepError = false } = {}) {
    if (!keepError) errorText = ''
    try {
      const [list, state_now] = await Promise.all([api('GET', '/proxies'), invoke('clash_state')])
      proxies = list?.proxies || {}
      info = { version: String(state_now?.version ?? ''), mixed: Number(state_now?.mixed ?? 0), controller: Number(state_now?.controller ?? 0) }
      sysproxy = !!state_now?.sysproxy
      try {
        const settings = await invoke('settings_get')
        subValue = String(settings?.settings?.clashSubscription ?? '')
      } catch {}
      if (!groups().some(g => g.name === selGroup)) selGroup = ''
    } catch (error) {
      errorText = String(error?.message ?? error ?? '')
      if (!proxies) proxies = {}
    }
    render()
  }

  async function pick(group, node) {
    try {
      await api('PUT', `/proxies/${enc(group)}`, { name: node })
      const fresh = await api('GET', '/proxies')
      proxies = fresh?.proxies || proxies
    } catch (error) {
      errorText = String(error?.message ?? error ?? '')
    }
    render()
  }

  // 组内测速：每个节点一次控制器延迟测试，4 个并发；单节点失败=Error。
  async function testGroup(group) {
    const nodes = group?.all || []
    busy = 'testing'
    render()
    let at = 0
    const worker = async () => {
      while (at < nodes.length) {
        const node = nodes[at++]
        try {
          await api('GET', `/proxies/${enc(node)}/delay?url=${enc('https://www.gstatic.com/generate_204')}&timeout=5000`)
        } catch {}
      }
    }
    await Promise.all([worker(), worker(), worker(), worker()])
    try {
      const fresh = await api('GET', '/proxies')
      proxies = fresh?.proxies || proxies
    } catch {}
    busy = ''
    render()
  }

  async function saveSubscription() {
    const sub = el?.querySelector('.cp-sub')
    if (!sub) return
    const url = sub.value.trim()
    try {
      await invoke('settings_set', { patch: { clashSubscription: url } })
      subValue = url
      await invoke('clash_stop')
      const ready = await invoke('clash_ensure', { origin: window.location.origin })
      info = { version: String(ready?.version ?? ''), mixed: Number(ready?.mixed ?? 0), controller: Number(ready?.controller ?? 0) }
      await refresh()
    } catch (error) {
      errorText = String(error?.message ?? error ?? '')
      render()
    }
  }

  async function updateSubscription() {
    busy = 'updating'
    render()
    try {
      await api('PUT', '/providers/proxies/sub')
      await refresh({ keepError: true })
    } catch (error) {
      errorText = String(error?.message ?? error ?? '')
      render()
    }
    busy = ''
  }

  function wire() {
    el.addEventListener('click', event => {
      const at = selector => event.target.closest(selector)
      const box = at('[data-clash-sysproxy]')
      if (box) {
        const enable = box.checked
        invoke('clash_sysproxy', { enable })
          .then(done => {
            sysproxy = !!done?.on
            render()
          })
          .catch(error => {
            errorText = String(error?.message ?? error ?? '')
            box.checked = !enable
            render()
          })
        return
      }
      if (at('[data-clash-sub-save]')) return void saveSubscription()
      if (at('[data-clash-sub-update]')) return void updateSubscription()
      if (at('[data-clash-advanced]')) {
        const url = info.controller ? `http://127.0.0.1:${info.controller}/ui/?hostname=127.0.0.1&port=${info.controller}` : 'http://127.0.0.1:9099/ui/'
        return void invoke('oflm_open_url', { url }).catch(error => console.log('[clash advanced]', error))
      }
      if (at('[data-clash-toggle]')) {
        const stopping = !!info.version
        invoke(stopping ? 'clash_stop' : 'clash_ensure', stopping ? undefined : { origin: window.location.origin })
          .then(() => refresh())
          .catch(error => {
            errorText = String(error?.message ?? error ?? '')
            render()
          })
        return
      }
      const groupTab = at('[data-group-tab]')
      if (groupTab) {
        selGroup = groupTab.dataset.groupTab
        render()
        return
      }
      const test = at('[data-clash-test]')
      if (test) {
        const group = groups().find(g => g.name === test.dataset.clashTest)
        if (group) return void testGroup(group)
      }
      const node = at('[data-node]')
      if (node) return void pick(node.dataset.group, node.dataset.node)
    })
    el.addEventListener('keydown', event => {
      if (event.key === 'Enter' && event.target.classList?.contains('cp-sub')) saveSubscription()
    })
  }

  // 页面进入：渲染骨架、拉数据重绘。
  function mount(target, translate) {
    if (translate) T = translate
    if (el !== target) {
      el = target
      wire()
      render()
    }
    refresh()
  }

  window.Clash = { mount }
})()
