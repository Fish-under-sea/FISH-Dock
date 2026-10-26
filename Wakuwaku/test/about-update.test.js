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
test('关于页即使安装版也显示独立检查更新入口',()=>{
 assert.match(about({may:false}), /data-update="check"/)
})
test('新版本提示与版本号同排，检查中不重复提交',()=>{
 const html=about({latest:{newer:true,version:'1.2.0'},checking:true})
 assert.match(html,/发现新版本/);assert.match(html,/1\.2\.0/);assert.match(html,/data-update="check"[^>]*disabled/)
})
