# 参数契约夹具（2.0.4 T-K）：从 examples/widgets/stock.ps1 提取的 param() 块。
# 被测契约：plugins.rs 的 PLUGINS 里每个参数名都必须出现在这里；示例脚本本身只作样例。
param(
  [string[]]$Symbol = @('AAPL'),
  [string[]]$Name = @(),
  [double]$Minutes = 5,
  [switch]$RedUp,
  [switch]$GreenUp,
  [int]$Port = 47213,
  [switch]$Once
)
