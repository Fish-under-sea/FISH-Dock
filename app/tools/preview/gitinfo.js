// Which code a build or a picture is of, as git says it, in a few words:
// 「主线 · 3ec87d9」, 「分支 simulator · 3ec87d9」, with 「 + 未提交的改动」 when
// the files differ from the commit. publish.js puts it on the playground's
// header; compare.js under each side of a pair.
const { execFileSync } = require('node:child_process')

const MAIN = new Set(['main', 'master'])

// The words for a branch (null: detached), a short commit and whether the
// files differ from it.
function labelOf({ branch, sha, dirty = false }) {
  const where = !branch || branch === 'HEAD' ? '' : MAIN.has(branch) ? '主线 · ' : `分支 ${branch} · `
  return `${where}${sha || '?'}${dirty ? ' + 未提交的改动' : ''}`
}

const git = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim()

// The checkout at cwd as it is now: its branch, commit, and whether its
// files differ (only paths given, if any).
function here(cwd, paths = []) {
  const sha = git(cwd, 'rev-parse', '--short', 'HEAD')
  const branch = git(cwd, 'rev-parse', '--abbrev-ref', 'HEAD')
  const dirty = git(cwd, 'status', '--porcelain', '--', ...paths) !== ''
  return { branch: branch === 'HEAD' ? null : branch, sha, dirty }
}

// A ref (a branch, a tag, a commit): its commit, and the branch it names, if it is one.
function ref(cwd, name) {
  const sha = git(cwd, 'rev-parse', '--short', `${name}^{commit}`)
  let branch = null
  try {
    branch = git(cwd, 'rev-parse', '--abbrev-ref', '--verify', '--quiet', name).replace(/^origin\//, '') || null
  } catch {}
  if (branch === 'HEAD') branch = null
  return { branch, sha, dirty: false }
}

// The label of the checkout at cwd, or null where there is no git.
function label(cwd, paths) {
  try {
    return labelOf(here(cwd, paths))
  } catch {
    return null
  }
}

module.exports = { labelOf, here, ref, label }
