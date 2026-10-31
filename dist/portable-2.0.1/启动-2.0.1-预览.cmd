@echo off
chcp 65001 >nul
rem ============================================================================
rem  FISH Dock 2.0.1 预览版（便携，双击即可）
rem
rem  这是预览版，不安装、不覆盖已装的 FISH Dock：
rem    · 程序：本目录下的 FISH-Dock.exe（独立副本）
rem    · 数据：..\..\build\review-home（关掉也不会自动删，可随时整个删掉重置）
rem    · 端口：从 48101 起自动找空闲（避开已装的 47213、旧预览 47413、47901）
rem    · 不碰 %USERPROFILE%\.fish-dock、%LOCALAPPDATA%\FISH Dock，也不改注册表/任务计划
rem    · display 硬校验为 island：不会接管你的任务栏
rem  关掉它：托盘图标 → 退出，或再跑一次 preview-start.ps1 -Stop
rem ============================================================================
where pwsh >nul 2>nul
if %errorlevel%==0 (
  pwsh -NoProfile -ExecutionPolicy Bypass -File "%~dp0preview-start.ps1" %*
) else (
  powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0preview-start.ps1" %*
)
if errorlevel 1 pause
