#!/usr/bin/env bash
cd "/c/Users/User/Projetos/Assistente/DeepSeek-Harness" || exit 1
DSH_HOME="/c/Users/User/Projetos/Assistente/DeepSeek-Harness/dsh_home" \
TEKA_PONTE_TOKEN=teste-manual-1234 \
  ./instalacao-funcionando/node_modules/.bin/dsh --profile teka \
  > /c/Users/User/Projetos/Assistente/Teka-IA/logs/dsh_manual.log 2>&1
