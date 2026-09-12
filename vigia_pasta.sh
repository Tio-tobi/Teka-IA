#!/usr/bin/env bash
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
while ! grep -aq 'PASTA COMPLETA' logs/pasta_corrida.log 2>/dev/null; do sleep 120; done
echo "=== fechou — $(date +%H:%M)" > logs/analise_pasta.log
python analisa_pasta.py >> logs/analise_pasta.log 2>&1
