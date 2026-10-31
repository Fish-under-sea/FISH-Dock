(() => {
  const esc = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]))
  function create({invoke,redraw,lang=()=> 'zh'}) {
    let view=null,busy=false,error='',message='',results=[],preview=null,collection=null,collectionResults=[],source='',sourceUrl='',path='',elevated=false,importToken=null
    const text=(zh,en)=>lang()==='zh'?zh:en
    async function run(fn) {
      if(busy)return
      busy=true;error='';message='';redraw()
      try {await fn()} catch(e) {error=e.message||e.detail||String(e)} finally {busy=false;redraw()}
    }
    async function refresh(){view=await invoke('windhawk_state');path=view.layout?.install||path}
    async function load(){return run(refresh)}
    async function select(value){return run(async()=>{view=await invoke('windhawk_select',{path:value});path=value;preview=null})}
    async function apply(changes,admin=false){return run(async()=>{results=[];results=await invoke('windhawk_apply',{changes,elevated:admin});await refresh()})}
    async function inspect(file) {
      return run(async()=>{
        preview=null;source=''
        if(!file.name.endsWith('.wh.cpp'))throw Error(text('请选择 .wh.cpp 源文件','Choose a .wh.cpp source file'))
        if(file.size>2097152)throw Error(text('源文件超过 2 MiB','Source exceeds 2 MiB'))
        const selected=await file.text()
        const checked=await invoke('windhawk_preview',{source:selected})
        source=selected;preview=checked
      })
    }
    async function fetchSource(url) {
      return run(async()=>{
        preview=null;source='';collection=null
        const answer=await invoke('windhawk_fetch_source',{url})
        source=answer.source;preview=answer.preview
      })
    }
    async function inspectCollection(file,sourceFiles) {
      return run(async()=>{
        collection=null;preview=null;source='';collectionResults=[]
        if(file.size>524288)throw Error(text('集合清单超过 512 KiB','Collection manifest exceeds 512 KiB'))
        const files=[];let bytes=0
        for(const item of Array.from(sourceFiles||[])) {
          bytes+=item.size||0
          if(files.length>=64||item.size>2097152||bytes>16777216)throw Error(text('集合源码超过大小或数量限制','Collection sources exceed size or count limits'))
          files.push({name:item.name,source:await item.text()})
        }
        collection=await invoke('windhawk_collection_preview',{manifest:await file.text(),files})
      })
    }
    async function installCollection() {
      return run(async()=>{
        if(!collection||!view?.installed)throw Error(text('请先预览集合并选择 Windhawk','Preview a collection and select Windhawk first'))
        const members=collection.mods.map(row=>({member:row.member,fingerprint:row.preview.fingerprint}))
        const answer=await invoke('windhawk_collection_import',{members})
        collectionResults=answer.results;collection=null
        await refresh()
      })
    }
    async function install() {
      return run(async()=>{
        if(!view?.installed||!preview?.fingerprint)throw Error(text('请先选择 Windhawk 并预览源文件','Select Windhawk and preview a source file first'))
        const answer=await invoke('windhawk_import',{source,fingerprint:preview.fingerprint})
        importToken=answer.token;preview=null;source=''
        await refresh()
        message=text('已编译并导入，当前保持停用。请确认设置后启用。','Compiled and imported, currently disabled. Review settings before enabling.')
      })
    }
    async function rollback(requestedToken) {
      return run(async()=>{
        const token=requestedToken||importToken||view?.lastImport?.token||null
        await invoke('windhawk_rollback',{token})
        importToken=null;preview=null;source=''
        await refresh()
        message=text('已恢复导入前的源码和配置。','Source and configuration restored to their previous state.')
      })
    }
    function render() {
      const off=busy?' disabled':''
      const rows=(view?.mods||[]).map(mod=>{
        const state=!mod.libraryFile?text('未编译','Not compiled'):mod.disabled?text('已停用','Disabled'):text('已启用','Enabled')
        return `<article class="r"><div class="grow"><strong>${esc(mod.metadata?.name||mod.id)}</strong><div class="note">${esc(mod.id)} · ${esc(mod.version)} · ${esc(mod.metadata?.author||'')}</div><p class="note">${esc(mod.metadata?.description||'')}</p><div class="note">${esc(mod.include||mod.metadata?.include?.join(', ')||'')} ${mod.exclude?` / ${esc(mod.exclude)}`:''}</div><span>${esc(state)}</span></div><button type="button" class="pbtn sm" data-windhawk-toggle="${esc(mod.id)}"${off||(!mod.libraryFile?' disabled':'')}>${esc(mod.disabled?text('启用','Enable'):text('停用','Disable'))}</button></article>`
      }).join('')
      const answers=results.map(row=>`<p ${row.ok?'role="status"':'class="bad" role="alert"'}>${esc(row.id)}：${esc(row.ok?text('已读取并确认状态','State verified'):row.error)}</p>`).join('')
      const recovery=importToken||view?.pendingImport||(view?.lastImport&&!view.lastImport.rolledBack)
      const previewHtml=preview?`<div class="grp"><strong>${esc(preview.metadata.name)}</strong><p>${esc(preview.metadata.id)} · ${esc(preview.metadata.version)} · ${esc(preview.metadata.author)}</p><p class="note">${esc(preview.metadata.description||'')}</p><p>${esc(preview.existing?`${text('现有版本','Current version')} ${preview.existing.version} → ${preview.metadata.version}；${text('将先备份现有源码和配置，保留已有设置。','Existing source and configuration will be backed up; existing settings are preserved.')}`:text('新 mod；编译成功后保持停用。','New mod; remains disabled after compilation.'))}</p><p class="note">${esc(text('编译最多等待 120 秒。编译失败时保留现有 mod。','Compilation may take up to 120 seconds. Existing mod is preserved on compilation failure.'))}</p><button type="button" class="pbtn" data-windhawk-import${off||(!view?.installed||!preview.fingerprint?' disabled':'')}>${esc(preview.existing?text('备份并编译替换','Back up, compile and replace'):text('编译并导入','Compile and import'))}</button></div>`:''
      const collectionHtml=collection?`<div class="grp"><strong>${esc(collection.name)}</strong>${collection.mods.map(row=>`<article><p>${esc(row.preview.metadata.name)} · ${esc(row.member.id)} · ${esc(row.preview.existing?.version||text('新安装','New'))} → ${esc(row.member.version)}</p><p class="note">${esc(text('将应用以下集合参数；原值保存在备份中。','The following presets will be applied; previous values are backed up.'))}</p><pre>${esc(JSON.stringify(row.member.settings,null,2))}</pre></article>`).join('')}<p>${esc(text('每个 mod 独立备份、编译和保持停用。失败逐项显示，成功项目可单独恢复。','Each mod is backed up and compiled independently, then kept disabled. Failures are reported separately; successful items can be restored individually.'))}</p><button type="button" class="pbtn" data-windhawk-collection-import${off||(!view?.installed?' disabled':'')}>${esc(text('确认编译集合与应用参数','Compile collection and apply presets'))}</button></div>`:''
      const collectionAnswers=(collectionResults.length?collectionResults:view?.collectionResults||[]).map(row=>`<article><p ${row.ok?'role="status"':'class="bad" role="alert"'}>${esc(row.id)}：${esc(row.ok?text('已导入，保持停用','Imported and disabled'):row.error)}</p>${row.ok&&row.answer?.token?`<button type="button" class="pbtn sm" data-windhawk-restore-token="${esc(row.answer.token)}"${off}>${esc(text('恢复此项备份','Restore this backup'))}</button>`:''}</article>`).join('')
      return `<section class="windhawk-panel grp" aria-labelledby="windhawk-title" aria-busy="${busy}">
        <header class="wh-head"><div><h3 id="windhawk-title">${esc(text('Windows 美化','Windows customization'))}<span class="wh-brand">Windhawk</span></h3><p class="note" role="status">${esc(view?view.bundled?text('内置引擎已就绪 · 导入模组即可使用','Built-in engine ready · Import a mod to get started'):view.installed?view.layout?.portable?text('便携引擎已就绪','Portable engine ready'):text('已连接外部引擎','External engine connected'):text('未检测到 Windhawk','Windhawk not found'):busy?text('正在检测内置引擎…','Checking built-in engine…'):text('打开后自动检测内置引擎','Built-in engine is checked when opened'))}</p></div><button type="button" class="pbtn sm" data-windhawk-refresh${off}>${esc(text('刷新','Refresh'))}</button></header>
        <div class="wh-mods">${rows||`<div class="wh-empty"><strong>${esc(text(view?.installed?'还没有导入模组':view?'内置引擎尚未就绪':'准备模组管理',view?.installed?'No mods imported yet':view?'Built-in engine unavailable':'Preparing mod manager'))}</strong><p class="note">${esc(text(view?.installed?'从下方导入源码或模组集合，编译后即可启用。':view?'请刷新重试；安装文件应包含引擎与编译器。':'引擎与编译器随软件提供，无需另行安装。',view?.installed?'Import a source file or collection below to compile and enable it.':view?'Refresh to retry. Installation resources should contain the engine and compiler.':'The engine and compiler are included with the application.'))}</p></div>`}</div>
        ${answers}${message?`<p role="status">${esc(message)}</p>`:''}${error?`<p class="bad" role="alert">${esc(error)}</p>`:''}
        ${recovery?`<div class="r"><p class="note">${esc(text(view?.pendingImport?'上次导入尚未完成，可恢复原配置。':'最近一次导入保留了源码与配置备份。',view?.pendingImport?'An import was interrupted. Restore its previous configuration.':'The most recent import has a source and configuration backup.'))}</p><button type="button" class="pbtn" data-windhawk-rollback${off}>${esc(text('恢复导入前备份','Restore previous backup'))}</button></div>`:''}
        <details${preview?' open':''}><summary>${esc(text('导入模组源码','Import mod source'))}</summary><label for="windhawk-source">${esc(text('选择 .wh.cpp 文件','Choose a .wh.cpp file'))}</label><input id="windhawk-source" type="file" accept=".cpp"${off}><label for="windhawk-url">${esc(text('远程源码 HTTPS 地址','Remote source HTTPS URL'))}</label><div class="r"><input class="in" id="windhawk-url" type="url" value="${esc(sourceUrl)}"${off}><button type="button" class="pbtn" data-windhawk-fetch${off}>${esc(text('下载并预览','Download and preview'))}</button></div>${previewHtml}</details>
        <details${collection?' open':''}><summary>${esc(text('导入模组集合','Import mod collection'))}</summary><p class="note">${esc(text('选择集合 JSON 清单；若引用本地源码，同时选择清单所列 .wh.cpp 文件。远程源码会按清单摘要校验。','Select a collection JSON manifest and its local .wh.cpp files, if referenced. Remote sources are checked against the manifest digest.'))}</p><label for="windhawk-collection">${esc(text('集合清单','Collection manifest'))}</label><input id="windhawk-collection" type="file" accept=".json"${off}><label for="windhawk-collection-sources">${esc(text('集合本地源码','Local collection sources'))}</label><input id="windhawk-collection-sources" type="file" multiple accept=".cpp"${off}><button type="button" class="pbtn" data-windhawk-collection-preview${off}>${esc(text('预览集合','Preview collection'))}</button>${collectionHtml}</details>${collectionAnswers}
        <details data-windhawk-advanced><summary>${esc(text('高级选项','Advanced options'))}</summary><p class="note">${esc(text('默认使用随软件提供的引擎，也可连接已有 Windhawk。','Use the included engine by default, or connect an existing Windhawk installation.'))}</p><label for="windhawk-path">${esc(text('安装或便携目录','Installation or portable directory'))}</label><div class="r"><input class="in" id="windhawk-path" type="text" value="${esc(path)}" spellcheck="false"${off}><button type="button" class="pbtn" data-windhawk-select${off}>${esc(text('使用此目录','Use directory'))}</button></div><label class="r"><input type="checkbox" data-windhawk-admin${elevated?' checked':''}${off}>${esc(text('本次开关使用系统提权','Elevate this change'))}</label><button type="button" class="pbtn sm" data-windhawk-open${off||(!view?.installed||view?.bundled?' disabled':'')}${!view?.installed||view?.bundled?' hidden':''}>${esc(text('打开外部 Windhawk 设置','Open external Windhawk settings'))}</button></details>
      </section>`
    }
    function mount(root) {
      const panel=root.querySelector('.windhawk-panel');if(!panel)return
      panel.querySelector('[data-windhawk-refresh]').onclick=()=>load()
      panel.querySelector('#windhawk-path').oninput=e=>{path=e.target.value}
      panel.querySelector('[data-windhawk-select]').onclick=()=>select(path)
      panel.querySelector('[data-windhawk-admin]').onchange=e=>{elevated=e.target.checked}
      panel.querySelector('[data-windhawk-open]').onclick=()=>run(()=>invoke('windhawk_open'))
      panel.querySelector('#windhawk-source').onchange=e=>{const file=e.target.files[0];if(file)void inspect(file)}
      const importButton=panel.querySelector('[data-windhawk-import]');if(importButton)importButton.onclick=()=>install()
      const rollbackButton=panel.querySelector('[data-windhawk-rollback]');if(rollbackButton)rollbackButton.onclick=()=>rollback()
      panel.querySelector('#windhawk-url').oninput=e=>{sourceUrl=e.target.value}
      panel.querySelector('[data-windhawk-fetch]').onclick=()=>fetchSource(sourceUrl)
      panel.querySelector('[data-windhawk-collection-preview]').onclick=()=>{const file=panel.querySelector('#windhawk-collection').files[0];if(file)void inspectCollection(file,panel.querySelector('#windhawk-collection-sources').files);else void run(()=>{throw Error(text('请选择集合清单','Choose a collection manifest'))})}
      const collectionButton=panel.querySelector('[data-windhawk-collection-import]');if(collectionButton)collectionButton.onclick=()=>installCollection()
      panel.querySelectorAll('[data-windhawk-restore-token]').forEach(button=>{button.onclick=()=>rollback(button.dataset.windhawkRestoreToken)})
      panel.querySelectorAll('[data-windhawk-toggle]').forEach(button=>{button.onclick=()=>{const mod=view.mods.find(m=>m.id===button.dataset.windhawkToggle);if(mod)void apply([{id:mod.id,disabled:!mod.disabled}],elevated)}})
    }
    return {load,select,apply,inspect,fetchSource,inspectCollection,installCollection,install,rollback,render,mount}
  }
  window.WindhawkPanel={create}
})()
