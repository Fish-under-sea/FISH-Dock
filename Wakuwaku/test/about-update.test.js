const assert = require('node:assert/strict')
const fs = require('node:fs')
const vm = require('node:vm')
const { test } = require('node:test')
const source = fs.readFileSync(require('node:path').join(__dirname, '../src/pet/settings.js'), 'utf8')
function about(update) {
 const start = source.indexOf('  const aboutHTML =')
 const end = source.indexOf('\n\n', start)
 return vm.runInNewContext(`${source.slice(start, end)};aboutHTML()`, {
  snap: { version: '1.1.0', update }, lang: 'zh', sec: x=>x, esc: x=>String(x).replace(/</g,'&lt;'), T: (k,v)=>k+(v?.version||'')
 })
}
const { t } = require('../src/shared/i18n.js')
function updatePanel(update) {
 const start=source.indexOf('  function pageUpdate()'), end=source.indexOf('  function pageCrash()',start)
 return vm.runInNewContext(`${source.slice(start,end)};pageUpdate()`,{
  snap:{version:'1.2.0',update,settings:{autoUpdateCheck:true}},lang:'zh',sec:x=>x,esc:x=>String(x),T:(key,vars)=>t('zh',key,vars),
  row:(title,note,actions)=>`<div>${title}|${note}|${actions}</div>`,sw:()=>'<switch>',seg:()=>'<segments>',netNow:()=>'',Date
 })
}
test('正式安装版显示发行安装模式、检查与新版下载入口，不再显示源码构建提示',()=>{
 const state={current:'1.2.0',may:false,mayCheck:true,installed:true,source:false,latest:{newer:true,version:'1.3.0',canInstall:false},checkedAt:Date.now()}
 const html=updatePanel(state)
 assert.doesNotMatch(html,/从源码运行|校验值|下载、校验并重启/)
 assert.match(html,/data-update="check"/);assert.match(html,/data-update="page"/);assert.match(html,/正式安装版/)
 assert.doesNotMatch(html,/data-update="update"/)
})
test('真正源码构建保留源码提示，正式安装版无更新时仍可手动检查',()=>{
 assert.match(updatePanel({may:false,mayCheck:false,source:true}),/从源码运行/)
 const html=updatePanel({may:false,mayCheck:true,installed:true,source:false,latest:{newer:false,version:'1.2.0'},checkedAt:Date.now()})
 assert.match(html,/已经是最新版本/);assert.match(html,/data-update="check"/);assert.doesNotMatch(html,/从源码运行/)
})
test('关于页双语标语采用一切皆插件并明确宿主与业务插件分工',()=>{
 assert.match(t('zh','home.intro'),/Everything is a Plugin.*一切皆插件/)
 assert.match(t('en','home.intro'),/Everything is a Plugin/)
 assert.match(t('zh','home.upstream'),/独立插件/)
})
test('关于页检查到当前最新版本后明确反馈检查成功',()=>{
 assert.match(about({checkedAt:Date.now(),latest:{newer:false,version:'1.2.0'}}),/s\.updateLatest/)
})
test('关于页即使安装版也显示独立检查更新入口',()=>{
 assert.match(about({may:false}), /data-update="check"/)
})
test('新版本提示与版本号同排，检查中不重复提交',()=>{
 const html=about({latest:{newer:true,version:'1.2.0'},checking:true})
 assert.match(html,/发现新版本/);assert.match(html,/1\.2\.0/);assert.match(html,/data-update="check"[^>]*disabled/)
})
