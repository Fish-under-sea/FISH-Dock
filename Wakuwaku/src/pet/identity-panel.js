// 身份 ZIP 只通过原生命令操作；页面不接触密码，也不接收任意导入路径。
;(function () {
  const escape = value => String(value ?? '').replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[char])
  function create({ invoke, confirm, lang, redraw }) {
    let view = null, preview = null, directory = '', edited = false, working = false, loaded = false, error = '', result = ''
    const text = (zh, en) => lang() === 'en' ? en : zh
    const run = async job => {
      if (working) return
      working = true; error = ''; result = ''; redraw()
      try { await job() } catch (failure) { error = typeof failure === 'string' ? failure : failure?.message || text('身份操作失败，请重试。', 'Identity operation failed. Please try again.') }
      finally { working = false; redraw() }
    }
    const load = async () => {
      if (loaded) return
      loaded = true
      try {
        view = await invoke('identity_view')
        if (!edited) directory = view.directory || ''
        error = view.error || view.directoryError || ''
      } catch (failure) { error = String(failure?.message || failure) }
      redraw()
    }
    const choose = () => run(async () => {
      preview = null
      const got = await invoke('identity_preview')
      if (!got.cancelled) preview = got
    })
    const save = async () => {
      if (working || !await confirm(text('导出的普通 ZIP 包含明文 API Key、渠道凭据和邮件授权码。请只保存在自己的可信位置。继续选择保存位置？', 'This ordinary ZIP includes plaintext API keys, provider credentials and mail passwords. Keep it in a trusted location. Continue to choose where to save it?'))) return
      return run(async () => {
        const got = await invoke('identity_export', { directory, confirm: true })
        if (got.saved) result = text('已保存：', 'Saved: ') + got.path + (got.missingCredentials ? text('；缺失凭据：', '; missing credentials: ') + got.missingCredentials : '')
      })
    }
    const commit = async () => {
      if (working || !preview) return
      if (!await confirm(text('将按预览范围恢复配置仓并保留恢复前副本。缺少的插件包需重新安装；旧备份只恢复原有范围。确认恢复？', 'Restore the previewed configuration and keep a before-copy? Reinstall missing extension packages. Legacy backups only restore their original scope.'))) return
      return run(async () => {
        const got = await invoke('identity_import', { ticket: preview.ticket, confirm: true })
        if (got.imported) {
          preview = null
          result = text('导入完成；原资料备份：', 'Imported. Previous files backed up at: ') + got.backup
          loaded = false
          await load()
        }
      })
    }
    function render() {
      const disabled = working ? ' disabled' : ''
      const missing = view?.missingMailCredentials || 0
      const configSummary = preview?.configuration ? `<p>${escape(text('将恢复宿主与插件：', 'Host and extensions to restore: '))}${preview.configuration.owners.map(owner => escape(owner.plugin_id + ' ' + owner.plugin_version + ' · schema ' + owner.data_schema)).join('、')}</p><p>${escape(text('持久文件：', 'Persistent files: '))}${preview.configuration.files}${escape(text('；程序包不包含在备份中，配置将保留供重新安装后使用。', '; packages are excluded. Configuration is kept for reinstall.'))}</p>` : ''
      const previewText = preview ? `<div class="identity-preview"><strong>${escape(preview.name)}</strong>${configSummary}<p>${escape(text('OFM 账号：', 'OFM accounts: '))}${preview.ofmAccounts} · ${escape(preview.mailAccounts === null ? text('邮件保持不变', 'Mail stays unchanged') : text('邮件账号：', 'Mail accounts: ') + preview.mailAccounts)}</p><p>${escape(text('缺失凭据：', 'Missing credentials: '))}${preview.missingCredentials}</p><button type="button" class="pbtn al" data-identity="import"${disabled}>${escape(text('确认恢复备份', 'Confirm restore'))}</button></div>` : ''
      return `<section class="identity-panel" aria-labelledby="identity-heading" aria-busy="${working}">
        <h3 id="identity-heading">${escape(text('FISH Dock 配置与备份', 'FISH Dock configuration and backups'))}</h3>
        <p class="note">${escape(text('备份整个配置仓：宿主设置、布局、插件注册与配置，以及插件凭据、名册和密钥。普通 ZIP 含明文凭据，导入后在本机重新加密。', 'Back up host settings, layout, extension registration, configuration, credentials and rosters. ZIPs contain plaintext secrets, re-encrypted locally on restore.'))}</p>
        ${view?.root ? `<p class="note identity-path">${escape(text('配置仓目录：', 'Configuration repository: '))}${escape(view.root)}</p>` : ''}
        ${missing ? `<p class="identity-warning">${escape(text('缺失邮件凭据：', 'Missing mail credentials: '))}${missing}${escape(text('；请在邮件设置重新填写。', '. Re-enter them in Mail settings.'))}</p>` : ''}
        <label class="identity-label" for="identity-directory">${escape(text('导出默认目录', 'Default export folder'))}</label>
        <input class="identity-directory" id="identity-directory" type="text" value="${escape(directory)}" autocomplete="off" spellcheck="false"${disabled}>
        <p class="note">${escape(text('默认保存在配置仓的 host/exports，可在此修改。备份排除临时文件、缓存、程序资源、可重新下载的插件包和历史备份；缺少插件包时需从商店重新安装。', 'Defaults to host/exports in the configuration repository. Edit it here. Temporary files, caches, program resources, downloadable extension packages and old backups are excluded; reinstall missing packages from the store.'))}</p>
        <div class="identity-actions"><button type="button" class="pbtn" data-identity="export"${disabled}>${escape(text('导出配置备份 ZIP', 'Export configuration ZIP'))}</button><button type="button" class="pbtn" data-identity="preview"${disabled}>${escape(text('选择备份 ZIP 并预览', 'Choose backup ZIP and preview'))}</button></div>
        ${previewText}
        ${working ? `<p role="status">${escape(text('正在处理，请完成文件窗口中的选择…', 'Working. Complete your selection in the file dialog…'))}</p>` : ''}
        ${error ? `<p class="identity-error" role="alert">${escape(error)}</p>` : ''}
        ${result ? `<p class="identity-result" role="status">${escape(result)}</p>` : ''}
      </section>`
    }
    function mount(root) {
      const panel = root.querySelector('.identity-panel')
      if (!panel) return
      const field = panel.querySelector('#identity-directory')
      field.oninput = () => { directory = field.value; edited = true }
      panel.querySelector('[data-identity="export"]').onclick = save
      panel.querySelector('[data-identity="preview"]').onclick = choose
      const importing = panel.querySelector('[data-identity="import"]')
      if (importing) importing.onclick = commit
      void load()
    }
    return { render, mount, load, preview: choose, export: save, import: commit, setDirectory(value) { directory = value; edited = true }, busy: () => working }
  }
  window.IdentityPanel = { create }
})()
