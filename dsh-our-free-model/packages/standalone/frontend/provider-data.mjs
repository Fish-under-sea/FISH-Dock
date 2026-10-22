/** FISH Dock 的厂商展示顺序；内部渠道标识与账号存储保持原样。 */
export function presentProviders(original) {
  const order = ['buddy', 'workbuddy', 'trae', 'cline', 'codearts', 'lobsterai', 'loomy', 'raccoon', 'meoo', 'minimax', 'qoder', 'qodercn', 'zcode', 'gemini']
  const names = { qoder: 'Qoder 国际版', qodercn: 'Qoder 国内版' }
  return [...original.filter(row => row.id !== 'meoo'), {
    id: 'meoo', name: 'Meoo', org: '阿里巴巴 · 秒悟', accent: '#8A66F5',
    note: '浏览器登录绑定已有项目的模型服务，每日积分。', login: 'browser',
  }].map(row => ({ ...row, name: names[row.id] ?? row.name }))
    .sort((a, b) => (order.includes(a.id) ? order.indexOf(a.id) : order.length) - (order.includes(b.id) ? order.indexOf(b.id) : order.length))
}
