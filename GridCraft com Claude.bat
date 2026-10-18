@echo off
rem Abre o GridCraft com o canal de controle na porta 7979, para o Claude (MCP "gridcraft") controlar a janela.
start "" "%~dp0target\release\gridcraft.exe" --control 7979 %*
