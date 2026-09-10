#!/usr/bin/env bash
# Envoltorio para lancar `exp_22.sh` DESTACADO da sessao do Claude.
# O `nohup ... &` de dentro do meu shell nao sobrevive ao meu reinicio.
cd /c/Users/User/Projetos/Assistente/Teka-IA || exit 1
exec ./exp_22.sh >> tr22_corrida.log 2>&1
