#!/usr/bin/env bash
# Envoltorio para lancar `exp_critico.sh` DESTACADO da sessao do Claude.
#
# O `nohup ... &` de dentro do meu shell nao sobrevive: quando a sessao reiniciou
# em 09/09, a corrida morreu junto e so a semente 19 tinha fechado. Este arquivo
# existe para o `Start-Process` do PowerShell ter um alvo simples, sem aspas
# aninhadas para o Windows mastigar.
cd /c/Users/User/Projetos/Assistente/Teka-IA || exit 1
exec ./exp_critico.sh >> cr_corrida.log 2>&1
