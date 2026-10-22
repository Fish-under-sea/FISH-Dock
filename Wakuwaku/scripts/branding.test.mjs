import assert from 'node:assert/strict'
import fs from 'node:fs'
import { createHash } from 'node:crypto'
import { createRequire } from 'node:module'
import test from 'node:test'

const root = new URL('../', import.meta.url)
const read = file => fs.readFileSync(new URL(file, root), 'utf8')
const config = JSON.parse(read('src-tauri/tauri.conf.json'))
const require = createRequire(import.meta.url)
const { t } = require('../src/shared/i18n.js')

test('应用元数据、通知与用户品牌一致，内部身份保持兼容', () => {
  assert.equal(config.productName, 'FISH Dock')
  assert.equal(config.identifier, 'com.fish.dshfish')
  assert.deepEqual(config.bundle.icon, ['icons/DSH-FISH-Windows.ico', 'icons/dsh-fish.png'])
  const notify = read('src-tauri/src/notify.rs')
  assert.ok(notify.includes(`pub const APP_ID: &str = "${config.identifier}";`))
  assert.ok(notify.includes('.title("FISH Dock")'))
})

test('中英文设置、托盘和版本文案使用 FISH Dock', () => {
  for (const lang of ['zh', 'en']) {
    for (const key of ['settings.title', 'tray.tooltip', 's.version']) {
      assert.match(t(lang, key, { status: 'idle', version: config.version }), /^FISH Dock/)
    }
  }
})

test('托盘、通知和 README 使用用户原图，不再引用自绘素材', () => {
  const source = read('src-tauri/src/tray.rs')
  assert.ok(source.includes('include_bytes!("../icons/dsh-fish.png")'))
  assert.ok(!source.includes('include_bytes!("../icons/tray/'))
  const notify = read('src-tauri/src/notify.rs')
  assert.ok(notify.includes('include_bytes!("../icons/dsh-fish.png")'))
  assert.ok(notify.includes('dir.join("fish-dock-user-icon-v1.png")'))
  for (const file of ['README.md', 'README.en.md']) {
    assert.ok(read(file).includes('src="src-tauri/icons/dsh-fish.png"'))
    assert.ok(!read(file).includes('art/fish-dock-master.png'))
  }
})

test('用户原始 ICO 与 PNG 保持不变，托盘直接复用 ICO 内的原图', () => {
  const ico = fs.readFileSync(new URL('src-tauri/icons/DSH-FISH-Windows.ico', root))
  assert.equal(ico.readUInt16LE(2), 1)
  const sizes = Array.from({ length: ico.readUInt16LE(4) }, (_, i) => {
    const at = 6 + i * 16
    const width = ico[at] || 256
    assert.equal(ico[at + 1] || 256, width)
    assert.ok(ico.readUInt32LE(at + 8) > 0)
    assert.ok(ico.readUInt32LE(at + 12) + ico.readUInt32LE(at + 8) <= ico.length)
    return width
  })
  assert.deepEqual(sizes.sort((a, b) => a - b), [16, 20, 24, 32, 40, 48, 64, 96, 128, 256])
  const png = fs.readFileSync(new URL('src-tauri/icons/dsh-fish.png', root))
  assert.equal(createHash('sha256').update(ico).digest('hex'), '5130fe9d4e381de4692446aa08729ad2de59cca443f7d40367fbcdff41fbfed0')
  assert.equal(createHash('sha256').update(png).digest('hex'), '5688f69de350b8c75e0832e7b5928c680a1b7faa14ecd0259306afbd5c4178b9')
  const entry = 6 + sizes.indexOf(256) * 16
  const start = ico.readUInt32LE(entry + 12)
  assert.deepEqual(png, ico.subarray(start, start + ico.readUInt32LE(entry + 8)))
  assert.equal(png.readUInt32BE(16), 256)
  assert.equal(png.readUInt32BE(20), 256)
})

test('1.0.2 更新版与双语关于页完整标明集成来源', () => {
  assert.equal(config.version, '1.0.2')
  for (const lang of ['zh', 'en']) {
    assert.match(t(lang, 'home.intro'), /FISH Dock/)
    assert.match(t(lang, 'home.upstream'), /Wakuwaku/)
    assert.match(t(lang, 'home.upstream'), /dsh-our-free-model/)
    assert.match(t(lang, 'home.upstream'), /Pengjie Zhou/)
    assert.match(t(lang, 'home.upstream'), /Ebony-Vinyl/)
  }
  const settings = read('src/pet/settings.js')
  for (const key of ['home.intro', 'home.upstream']) assert.ok(settings.includes(`T('${key}')`))
})

test('项目入口归属自有仓库，不向原项目上传崩溃报告', () => {
  assert.match(read('src-tauri/src/settings.rs'), /const REPO: &str = "https:\/\/github.com\/Fish-under-sea\/FISH-Dock"/)
  const crash = read('src-tauri/src/crash.rs')
  for (const relay of ['GITHUB_RELAY', 'GITEE_RELAY']) assert.ok(crash.includes(`const ${relay}: &str = "";`))
  assert.match(read('src-tauri/src/updates.rs'), /https:\/\/api.github.com\/repos\/Fish-under-sea\/FISH-Dock\/releases\/latest/)
})
