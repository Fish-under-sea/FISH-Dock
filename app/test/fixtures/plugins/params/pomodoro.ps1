# 参数契约夹具（2.0.4 T-K）：从 examples/widgets/pomodoro.ps1 提取的 param() 块。
# 被测契约：plugins.rs 的 PLUGINS 里每个参数名都必须出现在这里；示例脚本本身只作样例。
param(
  [double]$Work = 25,
  [double]$Break = 5,
  [double]$LongBreak = 15,
  # A long break after every so many rounds.
  [int]$Every = 4,
  # Rounds before it stops; 0, until Ctrl+C.
  [int]$Rounds = 0,
  [int]$Port = 47213
)
