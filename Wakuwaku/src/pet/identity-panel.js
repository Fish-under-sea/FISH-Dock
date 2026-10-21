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
      if (working || !confirm(text('导出的普通 ZIP 包含明文 API Key、渠道凭据和邮件授权码。请只保存在自己的可信位置。继续选择保存位置？', 'This ordinary ZIP includes plaintext API keys, provider credentials and mail passwords. Keep it in a trusted location. Continue to choose where to save it?'))) return
      return run(async () => {
        const got = await invoke('identity_export', { directory, confirm: true })
        if (got.saved) result = text('已保存：', 'Saved: ') + got.path + (got.missingCredentials ? text('；缺失凭据：', '; missing credentials: ') + got.missingCredentials : '')
      })
    }
    const commit = async () => {
      if (working || !preview) return
      if (!confirm(text('将替换预览所示身份资料，并保留替换前备份。旧 OFM 包不替换邮件。确认导入？', 'Replace the identities shown in the preview and keep a backup of the current files? Older OFM backups leave mail unchanged.'))) return
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
      const previewText = preview ? `<div class="identity-preview"><strong>${escape(preview.name)}</strong><p>${escape(text('OFM 账号：', 'OFM accounts: '))}${preview.ofmAccounts} · ${escape(preview.mailAccounts === null ? text('邮件保持不变', 'Mail stays unchanged') : text('邮件账号：', 'Mail accounts: ') + preview.mailAccounts)}</p><p>${escape(text('缺失凭据：', 'Missing credentials: '))}${preview.missingCredentials}</p><button type="button" class="pbtn al" data-identity="import"${disabled}>${escape(text('确认替换身份', 'Confirm replacement'))}</button></div>` : ''
      return `<section class="identity-panel" aria-labelledby="identity-heading" aria-busy="${working}">
        <h3 id="identity-heading">${escape(text('身份与备份', 'Identity and backups'))}</h3>
        <p class="note">${escape(text('统一保存 OFM 账号、密钥和邮件身份。导出的普通 ZIP 含明文凭据；邮件在本机由 Windows 加密保存。', 'OFM accounts, keys and mail identities are stored together. Exported ZIPs contain plaintext credentials; local mail passwords use Windows encryption.'))}</p>
        ${view?.root ? `<p class="note identity-path">${escape(text('身份目录：', 'Identity folder: '))}${escape(view.root)}</p>` : ''}
        ${missing ? `<p class="identity-warning">${escape(text('缺失邮件凭据：', 'Missing mail credentials: '))}${missing}${escape(text('；请在邮件设置重新填写。', '. Re-enter them in Mail settings.'))}</p>` : ''}
        <label class="identity-label" for="identity-directory">${escape(text('导出默认目录', 'Default export folder'))}</label>
        <input class="identity-directory" id="identity-directory" type="text" value="${escape(directory)}" autocomplete="off" spellcheck="false"${disabled}>
        <p class="note">${escape(text('首次使用系统「下载」目录；可在此编辑，保存时仍可选择文件位置。', 'Starts with your system Downloads folder. Edit it here or choose another location in the save dialog.'))}</p>
        <div class="identity-actions"><button type="button" class="pbtn" data-identity="export"${disabled}>${escape(text('导出身份 ZIP', 'Export identity ZIP'))}</button><button type="button" class="pbtn" data-identity="preview"${disabled}>${escape(text('选择 ZIP 并预览', 'Choose ZIP and preview'))}</button></div>
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
