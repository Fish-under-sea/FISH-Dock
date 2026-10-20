// Sample data for the scenes, as the Rust side would send it: sessions in
// each mood (state.rs view), a prompt (asks.rs view), widgets with their
// details and sizes (widgets.rs), the plugins (plugins.rs view), the
// taskbar's programs, tray and keyboard (taskbar.rs), and desktop layouts
// (desk.rs). Loaded after fake-tauri.js (its clock, Fake.T0). Node reads it
// too (the tests of scenes.js): there the icons are a placeholder.
;(function (root) {
  const T0 = root.Fake?.T0 ?? Date.now()
  const canDraw = typeof document !== 'undefined'
  const NO_ICON = 'data:image/png;base64,'

  // A program's icon as Windows gives one (a PNG): a rounded square in its
  // colour with its letter, drawn here.
  function appIcon(color, letter, size = 48) {
    if (!canDraw) return NO_ICON
    const c = document.createElement('canvas')
    c.width = c.height = size
    const g = c.getContext('2d')
    const grad = g.createLinearGradient(0, 0, size, size)
    grad.addColorStop(0, color)
    grad.addColorStop(1, shade(color, -0.25))
    g.fillStyle = grad
    g.beginPath()
    g.roundRect(size * 0.08, size * 0.08, size * 0.84, size * 0.84, size * 0.2)
    g.fill()
    g.fillStyle = '#fff'
    g.font = `600 ${Math.round(size * 0.46)}px "Segoe UI Variable Display", "Segoe UI", sans-serif`
    g.textAlign = 'center'
    g.textBaseline = 'middle'
    g.fillText(letter, size / 2, size / 2 + size * 0.03)
    return c.toDataURL()
  }
  // A tray icon: a small round mark in its colour.
  function trayIcon(color, letter, size = 32) {
    if (!canDraw) return NO_ICON
    const c = document.createElement('canvas')
    c.width = c.height = size
    const g = c.getContext('2d')
    g.fillStyle = color
    g.beginPath()
    g.arc(size / 2, size / 2, size * 0.44, 0, Math.PI * 2)
    g.fill()
    g.fillStyle = '#fff'
    g.font = `700 ${Math.round(size * 0.5)}px "Segoe UI", sans-serif`
    g.textAlign = 'center'
    g.textBaseline = 'middle'
    g.fillText(letter, size / 2, size / 2 + 1)
    return c.toDataURL()
  }
  function shade(hex, by) {
    const n = parseInt(hex.slice(1), 16)
    const f = v => Math.max(0, Math.min(255, Math.round(v + v * by)))
    return `rgb(${f(n >> 16)}, ${f((n >> 8) & 255)}, ${f(n & 255)})`
  }

  // --- Sessions ---------------------------------------------------------------------------
  const BASE = { id: 's1', project: 'pet', agent: 'claude', jump: true, name: '任务栏设计', detail: null, at: T0, since: null, took: null, step: null, stepSince: null, todo: null, files: 0, reply: '', error: null }
  const MOODS = {
    idle: {},
    working: { since: T0 - 95e3, step: { key: 'step.edit', vars: { file: 'island.js' } }, stepSince: T0 - 4e3, todo: { done: 3, total: 7, active: '胶囊的流光' } },
    waiting: { since: T0 - 131e3, detail: { key: 'detail.needsApproval', vars: { tool: 'Bash' } } },
    done: { took: 151e3, reply: '改好了：浅色下胶囊的描边，和小点的位置。' },
    review: { took: 151e3, files: 3, reply: '改了三个文件，来看看。' },
    error: { took: 42e3, error: { key: 'error.rate_limit', vars: {} } },
  }
  const session = (mood, extra = {}) => ({ ...BASE, mood, ...MOODS[mood], ...extra })
  // Others beside the one shown: Codex at work, a turn done.
  const others = () => [
    { ...BASE, id: 's2', agent: 'codex', name: '邮件同步', project: 'mail', mood: 'working', since: T0 - 412e3, step: { key: 'step.command', vars: { what: 'cargo test' } }, stepSince: T0 - 30e3 },
    { ...BASE, id: 's3', name: '发布说明', project: 'pet', mood: 'done', took: 73e3, reply: '写好了。' },
  ]
  // A third at work: reading, in another project.
  const reading = () => ({ ...BASE, id: 's3', name: '周报', project: 'docs', mood: 'working', since: T0 - 48e3, step: { key: 'step.read', vars: { file: 'style.md' } }, stepSince: T0 - 2e3 })

  // A permission prompt (asks.rs view).
  const ASK = { id: 1, kind: 'permission', tool: 'Bash', summary: 'npm test -- --watch=false', project: 'pet', session: 's1', always: { rules: ['Bash(npm test:*)'], dirs: [], modes: [], where: ['localSettings'] } }
  // DeepSeek Harness waiting on you (her plugin's, asks.rs views): its
  // sandbox to widen for a write outside the workspace, with its reason in
  // both languages; and its ask_user_question, as the test machine had them.
  const dsh = detail => ({ ...BASE, id: 's4', agent: 'dsh', project: 'default-workspace', name: '写个工作区外的文件', mood: 'waiting', since: T0 - 21e3, detail })
  const DSH_ASK = {
    id: 2,
    kind: 'permission',
    from: 'dsh',
    tool: 'Write',
    summary: 'C:\\Users\\mira\\notes\\note.txt',
    project: 'default-workspace',
    session: 's4',
    always: null,
    reason: { en: 'Allow this operation with danger-full-access permissions: the file is outside the workspace, so the sandbox must be widened for this one write.', zh: '允许本次操作使用 danger-full-access 权限：the file is outside the workspace, so the sandbox must be widened for this one write.' },
  }
  const DSH_QUESTION = {
    id: 3,
    kind: 'question',
    from: 'dsh',
    tool: 'AskUserQuestion',
    project: 'default-workspace',
    session: 's4',
    title: '',
    canAnswer: true,
    questions: [{ kind: 'choice', header: 'File Name', question: 'Which file name should be used: test1 or test2?', description: '', multiSelect: false, options: [{ label: 'test1', description: 'Use test1 as the file name.' }, { label: 'test2', description: 'Use test2 as the file name.' }] }],
  }

  // --- Mail (mail.rs) ---------------------------------------------------------------------
  // Two accounts as the settings keep them (mail.rs Account json), each with
  // how its sync is going (sync.rs tell: the unread in its inbox, how much is
  // kept whole on this computer, the proxy it went through), and what each
  // keeps (store.rs): its inbox and its Sent folder, by UID in the order the
  // letters came. A personal one (a1, whose widget inbox-a1 is), through
  // Windows' proxy as the Mail page has it; one for work (a2), straight to
  // the company's server (its own way: none), set up lately, its letters
  // still being kept whole. The letters' times are some hours before the
  // clock (T0).
  const ago = h => T0 - h * 3600e3
  const who = (name, address) => ({ name, address })
  const MIRA = who('林米拉', 'mira@example.org')
  const MIRA_WORK = who('Mira Lin', 'mira.lin@example.com')
  // Who writes to her: friends and the services she uses, and her colleagues.
  const PEOPLE = {
    anna: who('Anna', 'anna@example.org'),
    hannah: who('Hannah', 'hannah@example.org'),
    lena: who('Lena', 'lena@example.org'),
    werner: who('Werner', 'werner@example.org'),
    estate: who('物业服务中心', 'service@example.org'),
    gym: who('FitOne 健身', 'hello@example.org'),
    rail: who('铁路客服', 'tickets@example.org'),
    club: who('周三读书会', 'club@example.org'),
    library: who('城市图书馆', 'library@example.org'),
    weekly: who('开源周刊', 'weekly@example.org'),
    drive: who('云盘', 'no-reply@example.org'),
    aliyun: who('Aliyun', 'aliyun-billing@example.com'),
    cloudflare: who('Cloudflare', 'cloudflare-noreply@example.com'),
    github: who('GitHub', 'github-notifications@example.com'),
    gitee: who('Gitee', 'gitee-noreply@example.com'),
    petra: who('Petra Schmidt', 'petra.schmidt@example.com'),
    tomas: who('Tomás García', 'tomas.garcia@example.com'),
    office: who('行政 · Office', 'office@example.com'),
    jonas: who('Jonas Becker', 'jonas.becker@example.com'),
    priya: who('Priya Nair', 'priya.nair@example.com'),
    chen: who('Chen Wei', 'chen.wei@example.com'),
    finance: who('财务部', 'finance@example.com'),
    ci: who('CI', 'ci@example.com'),
    design: who('设计组', 'design@example.com'),
    everyone: who('全体', 'all@example.com'),
  }
  // A letter as kept: its UID, when it came (hours before the clock), who
  // from, what about; to whom (her, unless said), its flags (seen, starred,
  // answered), its Message-ID (made from its UID, unless said) and what it
  // answers (In-Reply-To, References), its text, its files.
  const letter = (to, uid, h, from, subject, more = {}) => ({ uid, date: ago(h), from, to: [to], cc: [], subject, id: `${uid}.mail@${from.address.split('@')[1]}`, parent: '', refs: [], seen: false, flagged: false, answered: false, text: '', files: [], ...more })
  const toHome = (...args) => letter(MIRA, ...args)
  const toWork = (...args) => letter(MIRA_WORK, ...args)
  // One of hers, in Sent (read).
  const sent = (me, uid, h, to, subject, more = {}) => ({ ...letter(to, uid, h, me, subject, more), seen: true })
  const file = (name, size, type = 'application/pdf') => ({ name, size, type })
  // What the letters a conversation names go by.
  const ID = {
    dinner: 'dinner-1@example.org',
    dinnerBack: 'dinner-2@example.org',
    christmas: 'christmas-1@example.org',
    pr52: 'wakuwaku/pull/52@example.com',
    school: 'qc-school-2027@example.org',
    move: 'migration-1@example.com',
    moveBack: 'migration-2@example.com',
    review: 'review-two-panes@example.com',
    reviewBack: 'review-two-panes-2@example.com',
    reviewPriya: 'review-two-panes-3@example.com',
  }
  const LETTERS = {
    a1: {
      inbox: [
        toHome(101, 264, PEOPLE.estate, '10 月停水通知', { text: '各位住户：\n\n因楼内供水管道检修，10 月 22 日（周四）9:00–15:00 全楼停水，请提前储水。\n\n给您带来不便，敬请谅解。\n物业服务中心' }),
        toHome(102, 216, PEOPLE.gym, '会员卡下月到期，现在续费 8 折', { text: '米拉你好：\n\n你的年卡 11 月 15 日到期。10 月底前续费享 8 折，前台或 App 里都能办。\n\nFitOne 健身' }),
        toHome(103, 204, PEOPLE.rail, '你的车票：Dresden Hbf → Berlin Hbf', { seen: true, flagged: true, files: [file('车票-4711-0815.pdf', 182340)], text: '订单 4711-0815\n\n10 月 24 日（周六）08:12 Dresden Hbf → 10:05 Berlin Hbf\n2 等座 · 7 号车厢 · 54 座\n\n车票在附件里，上车时出示即可。祝旅途愉快！' }),
        toHome(104, 192, PEOPLE.hannah, '上个月爬山的照片', { files: [file('萨克森小瑞士.zip', 19502431, 'application/zip')], text: '米拉：\n\n照片终于整理好了，挑了 24 张，打包在附件里。那天早上的云海太好看了！\n\n下次叫上 Anna 一起去。\nHannah' }),
        toHome(105, 168, PEOPLE.club, '十月读什么：《看不见的城市》', { text: '各位书友：\n\n十月读卡尔维诺的《看不见的城市》，10 月 28 日（周三）晚上 7 点老地方见。\n\n想带点心的在群里说一声。' }),
        toHome(106, 144, PEOPLE.library, '你借的《流畅的 Python》10 月 24 日到期', { text: '林米拉读者：\n\n你借的《流畅的 Python（第 2 版）》10 月 24 日到期，可以在网上续借一次（4 周）。\n\n城市图书馆' }),
        toHome(107, 132, PEOPLE.weekly, '开源周刊 · 第 120 期', { text: '本期：\n- 一个用 Rust 写的 IMAP 库发布了 2.0\n- 桌面宠物也能当任务栏？\n- 十个值得一看的 Tauri 项目\n\n退订请回复「退订」。' }),
        toHome(108, 120, PEOPLE.drive, '你的云盘空间快满了（92%）', { text: '你的 50 GB 空间已用 46 GB，大部分是照片和视频。可以清理一下，或者升级到 200 GB。' }),
        toHome(109, 108, PEOPLE.lena, '圣诞节回不回家？', { id: ID.christmas, seen: true, answered: true, text: '米拉！\n\n妈妈问你圣诞节回不回来，她想早点准备圣诞晚餐 😄 回来的话告诉我哪天到，我去车站接你。\n\nLena' }),
        toHome(110, 100, PEOPLE.aliyun, '函数计算 9 月账单', { files: [file('账单-2026-09.pdf', 96512)], text: '尊敬的用户：\n\n你的函数计算 9 月账单已出：¥3.42。明细见附件，也可以在费用中心查看。' }),
        toHome(111, 80, PEOPLE.cloudflare, 'Your Worker relay was deployed', { text: 'Your Worker "relay" was deployed successfully.\n\nVersion: 3f9c1a2\n\nView it in your dashboard.' }),
        toHome(112, 52, PEOPLE.github, '[wakuwaku] 邮件页的双栏 (#52)', { id: ID.pr52, seen: true, text: 'Kai opened a pull request: #52 邮件页的双栏\n\n列表和信并排，像 Thunderbird 那样；屏幕窄时还是一栏。\n\n— Reply to this email directly or view it on GitHub.' }),
        toHome(113, 31, PEOPLE.anna, '这周找个时间吃饭？', { id: ID.dinner, seen: true, answered: true, text: '米拉：\n\n好久没见了！这周找个时间吃饭？周四或周五晚上我都行。\n\nAnna' }),
        toHome(114, 30, PEOPLE.gitee, '仓库 wakuwaku 收到一个新的 Star', { text: '你好，林米拉：\n\n用户 kirin 给你的仓库 wakuwaku 点了一个 Star，现在一共 128 个。' }),
        toHome(115, 26, PEOPLE.werner, 'Summer School on Quantum Computing 2027', { id: ID.school, flagged: true, files: [file('poster.pdf', 1284391)], text: 'Dear Mira,\n\nwe are happy to announce the Summer School on Quantum Computing, 5–16 July 2027: lectures, hands-on labs and a poster session. Travel grants are available.\n\nApplications are open until 31 January. The poster is attached; please share it with your group.\n\nBest regards,\nWerner' }),
        toHome(116, 20, PEOPLE.github, 'Re: [wakuwaku] 邮件页的双栏 (#52)', { seen: true, parent: ID.pr52, refs: [ID.pr52], text: '@mira 看了一下：窄屏下切回一栏时，打开的信要留着。其他都好。\n\n— Reply to this email directly or view it on GitHub.' }),
        toHome(117, 5, PEOPLE.github, '[wakuwaku] #52 merged into main', { parent: ID.pr52, refs: [ID.pr52], text: 'Merged #52 into main.\n\n— Reply to this email directly or view it on GitHub.' }),
        toHome(118, 1, PEOPLE.anna, '周五见：我订好了那家餐厅', { parent: ID.dinnerBack, refs: [ID.dinner, ID.dinnerBack], text: '太好了！我订了周五晚上 7 点老城那家越南餐厅，四个人，Hannah 也来。\n\n周五见！\nAnna' }),
      ],
      sent: [
        sent(MIRA, 21, 96, PEOPLE.lena, 'Re: 圣诞节回不回家？', { parent: ID.christmas, refs: [ID.christmas], text: '回！23 号晚上到，坐 18:40 那班车。别让妈妈准备太多 😄\n\n米拉' }),
        sent(MIRA, 22, 29, PEOPLE.anna, 'Re: 这周找个时间吃饭？', { id: ID.dinnerBack, parent: ID.dinner, refs: [ID.dinner], text: '好呀！周五晚上吧，你挑地方？\n\n米拉' }),
      ],
    },
    a2: {
      inbox: [
        toWork(201, 120, PEOPLE.petra, '欢迎 Lukas 加入设计组', { to: [PEOPLE.design], seen: true, text: '大家好：\n\nLukas 下周一（10 月 19 日）加入设计组，负责桌面端的小工具。他的座位在 Jonas 旁边，请大家多帮忙。\n\nPetra' }),
        toWork(202, 118, PEOPLE.tomas, '服务器迁移时间表', { id: ID.move, seen: true, answered: true, text: 'Mira，\n\n邮件服务器打算 10 月 24 日（周六）晚上迁到新机房，停机大约两小时。设计组那边周末有没有要发的东西？\n\nTomás' }),
        toWork(203, 98, PEOPLE.office, '咖啡机修好了 ☕', { to: [PEOPLE.everyone], seen: true, text: '三楼的咖啡机修好了，新换了磨豆机。用完请清一下奶泡管 🙏' }),
        toWork(204, 74, PEOPLE.jonas, '设计评审：邮件页双栏', { id: ID.review, cc: [PEOPLE.priya], seen: true, answered: true, files: [file('mail-two-panes.png', 842113, 'image/png')], text: 'Mira、Priya：\n\n附件是邮件页双栏的稿子：左边列表，右边读信；窄于 820 px 时回到一栏。周四下午评审，先看看有没有大问题。\n\nJonas' }),
        toWork(205, 50, PEOPLE.priya, 'Re: 设计评审：邮件页双栏', { id: ID.reviewPriya, to: [PEOPLE.jonas, MIRA_WORK], parent: ID.review, refs: [ID.review], seen: true, text: '列表那边的未读点能不能再明显一点？浅色下几乎看不见。别的我都同意。\n\nPriya' }),
        toWork(206, 48, PEOPLE.chen, '邀请：Q4 规划会 · 10 月 21 日（周三）10:00', { to: [PEOPLE.design], seen: true, flagged: true, files: [file('invite.ics', 2140, 'text/calendar')], text: 'Q4 规划会\n时间：10 月 21 日（周三）10:00–11:30\n地点：大会议室，也可以线上\n\n请各组带上下个季度最要紧的三件事。' }),
        toWork(207, 27, PEOPLE.finance, '报销单 R-2026-0917 已批准', { seen: true, files: [file('R-2026-0917.pdf', 64230)], text: '你的报销单 R-2026-0917（德累斯顿差旅，€412.60）已批准，随 10 月工资发放。' }),
        toWork(208, 26, PEOPLE.chen, '周报 · 第 42 周', { to: [PEOPLE.design], seen: true, text: '本周：\n- 邮件页双栏进入评审\n- 小工具的设计规范定稿\n- Lukas 下周一入职\n\n下周：v0.4 发布（周二），Q4 规划会（周三）。' }),
        toWork(209, 20, PEOPLE.jonas, 'Re: 设计评审：邮件页双栏', { cc: [PEOPLE.priya], parent: ID.reviewPriya, refs: [ID.review, ID.reviewPriya], text: '好主意：未读点改成强调色，浅色下再加深一点。周四之前我更新一版稿子。\n\nJonas' }),
        toWork(210, 3, PEOPLE.ci, 'CI 失败：main #1288（desk.rs）', { text: 'main #1288 失败\n\n提交 7d41e0b · test desk::tests::wheel_goes_to_the_icon_view ... FAILED\n\n日志在 CI 的页面上。' }),
        toWork(211, 2, PEOPLE.tomas, 'Re: 服务器迁移时间表', { flagged: true, parent: ID.moveBack, refs: [ID.move, ID.moveBack], text: 'Mira，\n\n迁移改到 10 月 31 日了（新机房的电还没通好）。到时候我提前一天再提醒大家。\n\nTomás' }),
        toWork(212, 0.8, PEOPLE.priya, '下周一的演示稿', { files: [file('演示-双栏邮件页.pptx', 5832100, 'application/vnd.openxmlformats-officedocument.presentationml.presentation')], text: 'Mira：\n\n周一给客户的演示稿在附件里，第 6–9 页是邮件页，你帮我看看截图对不对？周日晚上之前给我就行。\n\n谢谢！\nPriya' }),
      ],
      sent: [
        sent(MIRA_WORK, 31, 116, PEOPLE.tomas, 'Re: 服务器迁移时间表', { id: ID.moveBack, parent: ID.move, refs: [ID.move], text: '周末我们没有要发的，按计划来吧。谢谢提前说！\n\nMira' }),
        sent(MIRA_WORK, 32, 72, PEOPLE.jonas, 'Re: 设计评审：邮件页双栏', { id: ID.reviewBack, cc: [PEOPLE.priya], parent: ID.review, refs: [ID.review], text: '看了，整体很好。两栏中间的分隔线可以再淡一点。周四见。\n\nMira' }),
      ],
    },
  }
  // The room a letter kept whole takes (its text, its files as sent: base64).
  const bytesOf = l => 1800 + l.text.length * 3 + l.files.reduce((n, f) => n + Math.ceil((f.size * 4) / 3), 0)
  const keptOf = (letters, have) => ({ have, total: letters.length, bytes: 9000 + letters.slice(0, have).reduce((n, l) => n + bytesOf(l), 0) })
  const allOf = id => [...LETTERS[id].inbox, ...LETTERS[id].sent]
  const unreadIn = id => LETTERS[id].inbox.filter(l => !l.seen).length
  const MAIL_ACCOUNTS = [
    {
      id: 'a1', address: MIRA.address, name: MIRA.name, host: 'imap.example.org', port: 993, security: 'ssl', username: MIRA.address, auth: 'auto',
      smtp: { host: 'smtp.example.org', port: 465, security: 'ssl', username: MIRA.address, auth: 'auto' }, on: true, proxy: null,
      status: { state: 'ok', unread: unreadIn('a1'), offline: keptOf(allOf('a1'), allOf('a1').length), via: '127.0.0.1:7890' },
    },
    {
      id: 'a2', address: MIRA_WORK.address, name: MIRA_WORK.name, host: 'mail.example.com', port: 993, security: 'ssl', username: 'mira.lin', auth: 'auto',
      smtp: { host: 'mail.example.com', port: 587, security: 'starttls', username: 'mira.lin', auth: 'auto' }, on: true, proxy: { mode: 'none' },
      status: { state: 'ok', unread: unreadIn('a2'), offline: keptOf(allOf('a2'), 9), via: null },
    },
  ]
  // A letter handed to Claude (agent.rs talk_view), by its key (its
  // Message-ID): the poster read too, what it said under the letter.
  const MAIL_TALKS = {
    [ID.school]: {
      key: ID.school, agent: 'claude', account: 'a1', mailbox: 'inbox', uid: 115, state: 'done', error: '',
      turns: [
        { who: 'start', text: '' },
        { who: 'tool', text: 'Read · letter.md' },
        { who: 'tool', text: 'Read · poster.pdf' },
        { who: 'agent', text: '**讲了什么**：Werner 邀请你参加 2027 年 7 月 5–16 日的量子计算暑期学校，有讲座、上机和海报环节，可以申请差旅补助。\n\n**要你做什么**：1 月 31 日前报名；他还请你把海报转给组里。\n\n**怎么回**：\n```\nDear Werner,\n\nthank you for the invitation! I will share the poster with my group and plan to apply before the deadline.\n\nBest regards,\nMira\n```' },
      ],
    },
  }
  // An account's widget as mail.rs show makes it from its inbox: the newest
  // unread letter (who: what about) its label, else its address; how many
  // are unread its value; the newest six unread its rows (by UID: as they came).
  function inboxWidget(account, inbox) {
    const unread = inbox.filter(l => !l.seen).sort((a, b) => b.uid - a.uid)
    const name = l => l.from.name || l.from.address
    const first = unread[0]
    return {
      id: `inbox-${account.id}`, from: 'mail', on: true, icon: 'mail', color: unread.length ? '#5e9bff' : '#8e8e93', private: true,
      label: first ? (first.subject ? `${name(first)}：${first.subject}` : name(first)) : account.address,
      value: unread.length ? { key: 'mail.unread', vars: { n: String(unread.length) } } : { key: 'mail.none' },
      detail: { sizes: ['s', 'm', 'l'], rows: unread.slice(0, 6).map(l => ({ from: name(l), subject: l.subject, at: l.date })) },
    }
  }
  // The mail world (fake-tauri.js keeps a copy): the accounts, what each
  // keeps, the talks; Windows' proxy, as mail would take it (the Mail page's
  // 跟随系统: Clash's, on).
  const MAIL = { accounts: MAIL_ACCOUNTS, letters: LETTERS, talks: MAIL_TALKS, systemProxy: { kind: 'http', address: '127.0.0.1:7890' } }

  // --- Widgets ----------------------------------------------------------------------------
  const hourNow = new Date(T0).getHours()
  const hours = Array.from({ length: 24 }, (_, h) => (h >= 9 && h <= hourNow ? [2, 5, 9, 14, 11, 18, 22, 25, 19, 23, 27, 22, 15, 9, 6][h - 9] || 4 : 0))
  const cpu = Array.from({ length: 30 }, (_, i) => 15 + Math.round(8 * Math.sin(i / 3) + (i % 5)))
  const net = [7400, 9000, 6000, 12000, 30000, 8000, 7400, 6600, 9100, 7400]
  const WIDGETS = [
    {
      id: 'monitor', builtIn: true, from: 'builtIn', on: true, icon: 'gauge', color: '#5e9bff', label: { key: 'widget.monitor' },
      value: { parts: [{ icon: 'cpu', text: '17%' }, { icon: 'memory', text: '44%' }, { icon: 'down', text: '7.4K' }, { icon: 'up', text: '337K' }] },
      detail: { sizes: ['s', 'm', 'l'], series: { points: cpu, unit: '%', hi: 100, lo: 0 }, rows: [{ kind: 'cpu', value: '17%', text: '3.4 GHz', name: '8', unit: '16' }, { kind: 'memory', value: '44%', text: '14.1 / 32 GB' }, { kind: 'gpu', value: '23%', name: 'RTX 4070', text: '52°' }, { kind: 'disk', value: '61%', name: 'Samsung SSD 990 PRO' }, { kind: 'net', value: '7.4K', text: '337K', points: net }, { kind: 'uptime', value: '273600' }] },
    },
    { id: 'today', builtIn: true, from: 'builtIn', on: true, icon: 'today', color: '#34d27b', label: { key: 'widget.today' }, value: { key: 'widget.todayValue', vars: { turns: '197', time: '2:01', approvals: '3' } }, detail: { sizes: ['s', 'm', 'l'], series: { points: hours, unit: 'turns' } } },
    { id: 'tokens', builtIn: true, from: 'builtIn', on: true, icon: 'code', color: '#ff9f0a', label: { key: 'widget.tokens' }, value: { key: 'widget.tokensValue', vars: { total: '1.7B', output: '3.4M' } } },
    {
      id: 'weather', plugin: 'weather', from: 'script', on: true, icon: 'weather', color: '#ffb340', label: 'Dresden · 阴', value: '11°',
      detail: {
        sizes: ['s', 'm', 'l'], extra: '8°~16°',
        series: { unit: '°', points: [9.5, 10.2, 11.8, 13.6, 15.1, 16.2, 16.0, 15.2, 14.0, 12.6, 11.4, 10.6, 10.0, 9.5, 9.1, 8.8, 8.6, 8.4, 8.3, 8.4, 8.9, 9.6, 10.4, 11.2] },
        rows: [{ kind: 'hour', when: '16时', icon: 'cloud', value: '11°' }, { kind: 'hour', when: '17时', icon: 'partly', value: '13°' }, { kind: 'hour', when: '18时', icon: 'sun', value: '15°' }, { kind: 'hour', when: '19时', icon: 'sun', value: '16°' }, { kind: 'hour', when: '20时', icon: 'cloud', value: '14°' }, { kind: 'hour', when: '21时', icon: 'cloud', value: '11°' }, { kind: 'hour', when: '22时', icon: 'moon', value: '9°' }, { kind: 'day', when: '明天', icon: 'rain', value: '9°~14°', name: '小雨' }, { kind: 'day', when: '后天', icon: 'cloud', value: '7°~12°', name: '阴' }],
      },
    },
    { id: 'stock-aapl', plugin: 'stock', from: 'script', on: true, icon: 'stock', color: '#ff5c6c', label: '苹果', value: '230.40 +0.80%', detail: { sizes: ['s', 'm', 'l'], series: { points: [228.6, 228.9, 229.4, 229.1, 229.8, 230.2, 229.9, 230.6, 230.1, 230.4, 230.0, 230.5, 230.8, 230.4], hi: 231.2, lo: 228.1 }, rows: [{ code: 'AAPL', price: '230.40', change: '+0.80%', value: '4,812 万' }] } },
    { id: 'stock-nvda', plugin: 'stock', from: 'script', on: true, icon: 'stock', color: '#34d27b', label: '英伟达', value: '141.20 −1.30%', detail: { sizes: ['s', 'm', 'l'], series: { points: [143.1, 142.6, 142.9, 142.0, 141.7, 141.9, 141.2] }, rows: [{ code: 'NVDA', price: '141.20', change: '−1.30%' }] } },
    // The personal mailbox's (MAIL a1): 13 unread, the newest from Anna.
    inboxWidget(MAIL_ACCOUNTS[0], LETTERS.a1.inbox),
    { id: 'pomodoro', plugin: 'pomodoro', from: 'script', on: true, icon: 'timer', color: '#ff5c6c', label: '专注', value: '18:42', detail: { sizes: ['s', 'm'], extra: '之后休息 5 分钟', series: { points: [1122], hi: 1500, lo: 0, unit: 's' }, rows: [{ kind: 'round', value: '3', unit: '4' }] } },
    { id: 'countdown', plugin: 'countdown', from: 'script', on: true, icon: 'flag', color: '#ffb340', label: 'v0.4 发布', value: '还有 3 天', detail: { sizes: ['s'], extra: '10 月 20 日 周二', rows: [{ kind: 'days', value: '3', unit: '天' }] } },
    // Scripts that sent no icon of their own: the first letter of their name
    // as their mark, one in its own colour, one with none (grey).
    { id: 'deploy', from: 'script', on: true, icon: 'dot', color: '#36cfc9', label: '部署', value: '成功', leftMs: 240e3 },
    { id: 'build', from: 'script', on: true, icon: 'dot', label: 'Build', value: '3:12', leftMs: 60e3 },
    // The work mailbox's (MAIL a2), last: the scenes that put up every
    // widget turned through them as before it came.
    inboxWidget(MAIL_ACCOUNTS[1], LETTERS.a2.inbox),
  ]
  const widget = id => WIDGETS.find(w => w.id === id)
  // Only these, in this order.
  const widgets = (...ids) => ids.map(widget).filter(Boolean)

  // The plugins she runs (the Plugins page): those on, and the others there to turn on.
  const PLUGINS = [
    { id: 'weather', icon: 'weather', on: true, state: 'running', params: [{ name: 'City', kind: 'text', required: false, value: 'Dresden' }] },
    { id: 'stock', icon: 'stock', on: true, state: 'running', params: [{ name: 'Symbol', kind: 'text', required: false, value: 'AAPL,NVDA' }, { name: 'Name', kind: 'text', required: false, value: '苹果,英伟达' }, { name: 'Colors', kind: 'choice', required: false, options: ['RedUp', 'GreenUp'], value: 'RedUp', chosen: false }] },
    { id: 'pomodoro', icon: 'timer', on: true, state: 'running', params: [{ name: 'Work', kind: 'number', required: false, value: '25' }, { name: 'Break', kind: 'number', required: false, value: '5' }] },
    { id: 'countdown', icon: 'flag', on: true, state: 'running', params: [{ name: 'Title', kind: 'text', required: true, value: 'v0.4 发布' }, { name: 'At', kind: 'text', required: true, value: '2026-10-20' }] },
    { id: 'ci', icon: 'check', on: false, state: 'off', params: [{ name: 'Repo', kind: 'text', required: true, value: '' }] },
    { id: 'devserver', icon: 'server', on: false, state: 'off', params: [{ name: 'Url', kind: 'text', required: true, value: '' }] },
    { id: 'stretch', icon: 'timer', on: false, state: 'off', params: [{ name: 'Minutes', kind: 'number', required: false, value: '50' }] },
  ]

  // What the plugins off above (PLUGINS) say once they are turned on (put
  // somewhere from the picker: fake-tauri.js widgetsNow).
  const SLEEPING = {
    ci: { id: 'ci-main', plugin: 'ci', from: 'script', on: true, icon: 'check', color: '#34d27b', label: 'wakuwaku · main', value: '通过', detail: { sizes: ['s', 'm'], extra: '3 分钟前' } },
    devserver: { id: 'devserver', plugin: 'devserver', from: 'script', on: true, icon: 'server', color: '#36cfc9', label: 'localhost:5173', value: '运行中' },
    stretch: { id: 'stretch', plugin: 'stretch', from: 'script', on: true, icon: 'timer', color: '#b18cff', label: '久坐提醒', value: '32 分钟后' },
  }

  // --- The taskbar ------------------------------------------------------------------------
  let apps = 0
  const app = (name, color, letter, titles, opts = {}) => {
    const n = ++apps
    const png = appIcon(color, letter)
    return {
      app: name.toLowerCase(),
      path: `C:\\Program Files\\${name}\\${name}.exe`,
      name,
      pinned: !!opts.pinned,
      png,
      windows: titles.map((title, i) => ({ id: 100 * n + i, title, png, front: !!opts.front && i === 0, min: false, max: false, flash: false, sessions: [] })),
    }
  }
  const WINDOWS = [
    app('Explorer', '#f2c23a', 'F', ['下载'], { pinned: true }),
    app('Edge', '#1f9be0', 'E', ['Wakuwaku · GitHub'], { pinned: true }),
    app('Terminal', '#3b3f4a', '>', ['pet — claude', 'mail — codex'], { front: true }),
    app('Code', '#2f80ed', 'C', ['island.js — pet']),
    app('Claude', '#d97757', '✳', ['Claude']),
    app('Notes', '#7a5af8', 'N', [], { pinned: true }),
  ]
  const TRAY = [
    { key: 1, name: 'onedrive', png: trayIcon('#2d7ff9', '☁'), tip: 'OneDrive', exe: 'OneDrive.exe', windowsOut: true },
    { key: 2, name: 'wechat', png: trayIcon('#2dc100', 'W'), tip: '微信', exe: 'Weixin.exe', windowsOut: true },
    { key: 3, name: 'steam', png: trayIcon('#1b2838', 'S'), tip: 'Steam', exe: 'steam.exe', windowsOut: false },
  ]
  const KEYS = { lang: 0x0804, native: true, caps: false, net: 'wifi', volume: [40, false], ime: { id: 'ms-pinyin', lang: '中文(简体，中国)', name: '微软拼音', own: true, mode: { tip: '中文', glyph: '中' } } }

  // --- The desktop (desk.rs layouts) ------------------------------------------------------
  const K = 'd1'
  const at = (c, r) => ({ d: K, at: { [K]: [c, r] } })
  const DESK = {
    // Each size of four kinds: a column each, large over medium over small.
    sizes: [
      { id: 'ws1', type: 'plugin', of: 'weather', size: 'l', ...at(0, 0) },
      { id: 'ws2', type: 'plugin', of: 'weather', size: 'm', ...at(0, 4) },
      { id: 'ws3', type: 'plugin', of: 'weather', size: 's', ...at(0, 6) },
      { id: 'wm1', type: 'widget', of: 'monitor', size: 'l', ...at(4, 0) },
      { id: 'wm2', type: 'widget', of: 'monitor', size: 'm', ...at(4, 4) },
      { id: 'wm3', type: 'widget', of: 'monitor', size: 's', ...at(4, 6) },
      { id: 'wt1', type: 'widget', of: 'today', size: 'l', ...at(8, 0) },
      { id: 'wt2', type: 'widget', of: 'today', size: 'm', ...at(8, 4) },
      { id: 'wt3', type: 'widget', of: 'today', size: 's', ...at(8, 6) },
      { id: 'wd1', type: 'widget', of: 'deploy', size: 'm', ...at(12, 0) },
      { id: 'wd2', type: 'widget', of: 'deploy', size: 's', ...at(12, 2) },
      { id: 'wd3', type: 'widget', of: 'build', size: 's', ...at(14, 2) },
    ],
    // One alone, a stack, a board open and one folded.
    scene: [
      { id: 'wa', type: 'plugin', of: 'weather', size: 'm', ...at(0, 0) },
      { id: 'wst', type: 'stack', size: 'm', ...at(0, 3), items: [{ type: 'widget', of: 'inbox-a1' }, { type: 'widget', of: 'today' }, { type: 'plugin', of: 'pomodoro' }] },
      {
        id: 'wbw', type: 'board', name: '工作', ...at(5, 0),
        items: [
          { type: 'widget', of: 'today', size: 'm', at: [0, 0] },
          { type: 'widget', of: 'inbox-a1', size: 'm', at: [4, 0] },
          { type: 'plugin', of: 'stock', size: 's', at: [0, 2] },
          { type: 'widget', of: 'monitor', size: 's', at: [2, 2] },
          { type: 'plugin', of: 'pomodoro', size: 's', at: [4, 2] },
          { type: 'plugin', of: 'countdown', size: 's', at: [6, 2] },
        ],
      },
      {
        id: 'wbf', type: 'board', name: '行情', folded: true, ...at(5, 6),
        items: [{ type: 'plugin', of: 'stock', size: 's', at: [0, 0] }, { type: 'plugin', of: 'weather', size: 's', at: [2, 0] }, { type: 'widget', of: 'deploy', size: 's', at: [4, 0] }],
      },
    ],
    // An everyday desktop (the playground's from the start): a stack, a
    // board, two alone at the far side.
    everyday: [
      { id: 'wds', type: 'stack', size: 'm', ...at(0, 0), items: [{ type: 'widget', of: 'inbox-a1' }, { type: 'plugin', of: 'pomodoro' }] },
      {
        id: 'wdb', type: 'board', name: '工作', ...at(0, 3),
        items: [{ type: 'widget', of: 'today', size: 'm', at: [0, 0] }, { type: 'widget', of: 'tokens', size: 's', at: [4, 0] }, { type: 'plugin', of: 'countdown', size: 's', at: [6, 0] }],
      },
      { id: 'wd1', type: 'widget', of: 'deploy', size: 's', ...at(16, 0) },
      { id: 'wd2', type: 'plugin', of: 'stock', size: 's', ...at(16, 2) },
    ],
  }
  // A full desktop: the scene's (one alone, a stack, two boards, one
  // folded) and two more alone at the far side.
  DESK.full = [...DESK.scene, { id: 'wfm', type: 'widget', of: 'monitor', size: 'm', ...at(14, 0) }, { id: 'wfd', type: 'widget', of: 'deploy', size: 's', ...at(14, 2) }, { id: 'wfb', type: 'widget', of: 'build', size: 's', ...at(16, 2) }]

  // --- Combos (settings boardSets: boards kept to put up again, the picker's 组合) ---------------
  const day = 24 * 3600e3
  const BOARD_SETS = [
    {
      id: 'scode',
      name: '写代码',
      items: [
        { type: 'widget', of: 'today', size: 'm', at: [0, 0] },
        { type: 'widget', of: 'tokens', size: 's', at: [4, 0] },
        { type: 'widget', of: 'deploy', size: 's', at: [6, 0] },
        { type: 'widget', of: 'monitor', size: 'm', at: [0, 2] },
        { type: 'plugin', of: 'pomodoro', size: 's', at: [4, 2] },
      ],
      saved: T0 - 3 * day,
    },
    {
      id: 'slazy',
      name: '摸鱼',
      items: [
        { type: 'plugin', of: 'weather', size: 'm', at: [0, 0] },
        { type: 'plugin', of: 'stock', size: 'm', at: [4, 0] },
        { type: 'widget', of: 'inbox-a1', size: 's', at: [0, 2] },
        { type: 'plugin', of: 'countdown', size: 's', at: [2, 2] },
      ],
      saved: T0 - day,
    },
  ]

  // --- The taskbar's layouts (taskbarLayout) -------------------------------------------------
  const PARTS = ['tray', 'keys', 'quick', 'clock']
  const TASKBAR = {
    // Windows 11's: her at the left, Start and the programs in the middle,
    // the monitor, the tray, the keyboard, quick settings and the clock.
    win11: { left: ['her'], center: ['start', 'apps'], right: ['monitor', ...PARTS] },
    // Widgets put on the strip: one medium, a carousel and a stack.
    widgets: {
      left: ['her'],
      center: ['start', 'apps'],
      right: [
        { id: 'wtw', type: 'plugin', of: 'weather', size: 'M', icon: 'weather' },
        { id: 'wtc', type: 'carousel', items: [{ type: 'widget', of: 'inbox-a1', icon: 'mail' }, { type: 'plugin', of: 'stock' }] },
        { id: 'wts', type: 'stack', items: [{ type: 'widget', of: 'monitor', icon: 'gauge' }, { type: 'widget', of: 'today', icon: 'today' }, { type: 'plugin', of: 'countdown' }] },
        ...PARTS,
      ],
    },
    // As full as one would make it: a medium by her; a medium, a carousel
    // and a stack at the right (more is wider than a 1600 px strip has room for).
    full: {
      left: ['her', { id: 'wfp', type: 'plugin', of: 'pomodoro', size: 'M', icon: 'timer' }],
      center: ['start', 'apps'],
      right: [
        { id: 'wfw', type: 'plugin', of: 'weather', size: 'M', icon: 'weather' },
        { id: 'wtc', type: 'carousel', items: [{ type: 'widget', of: 'inbox-a1', icon: 'mail' }, { type: 'plugin', of: 'stock' }] },
        { id: 'wts', type: 'stack', items: [{ type: 'widget', of: 'today', icon: 'today' }, { type: 'widget', of: 'monitor', icon: 'gauge' }, { type: 'widget', of: 'tokens', icon: 'code' }] },
        ...PARTS,
      ],
    },
  }

  const api = { T0, appIcon, trayIcon, session, others, reading, ASK, dsh, DSH_ASK, DSH_QUESTION, MAIL, inboxWidget, WIDGETS, widget, widgets, PLUGINS, SLEEPING, WINDOWS, TRAY, KEYS, DESK, TASKBAR, BOARD_SETS }
  if (typeof module !== 'undefined' && module.exports) module.exports = api
  else root.Fixtures = api
})(typeof window !== 'undefined' ? window : globalThis)
