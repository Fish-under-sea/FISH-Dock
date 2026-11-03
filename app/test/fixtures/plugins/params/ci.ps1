# 参数契约夹具（2.0.4 T-K）：从 examples/widgets/ci.ps1 提取的 param() 块。
# 被测契约：plugins.rs 的 PLUGINS 里每个参数名都必须出现在这里；示例脚本本身只作样例。
param(
  [Parameter(Mandatory)][string]$Repo,
  [string]$Branch = 'main',
  [switch]$Prs,
  [string]$Author = '',
  [double]$Minutes = 3,
  [string]$Token = $env:GITHUB_TOKEN,
  [int]$Port = 47213,
  [switch]$Once
)
