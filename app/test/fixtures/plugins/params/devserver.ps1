# 参数契约夹具（2.0.4 T-K）：从 examples/widgets/devserver.ps1 提取的 param() 块。
# 被测契约：plugins.rs 的 PLUGINS 里每个参数名都必须出现在这里；示例脚本本身只作样例。
param(
  [string[]]$Url = @('http://localhost:3000'),
  [int]$Seconds = 15,
  [int]$Port = 47213,
  [switch]$Once
)
