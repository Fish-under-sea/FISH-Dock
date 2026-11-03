# 参数契约夹具（2.0.4 T-K）：从 examples/widgets/weather.ps1 提取的 param() 块。
# 被测契约：plugins.rs 的 PLUGINS 里每个参数名都必须出现在这里；示例脚本本身只作样例。
param(
  [string]$City = 'Shanghai',
  [int]$Port = 47213,
  [int]$Minutes = 10,
  [switch]$Once,
  # zh or en: Chinese if Windows' language or its formats are.
  [string]$Lang = $(if ((Get-UICulture).Name -like 'zh*' -or (Get-Culture).Name -like 'zh*') { 'zh' } else { 'en' })
)
