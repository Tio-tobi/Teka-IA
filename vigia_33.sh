#!/usr/bin/env bash
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
while ! grep -aq 'POCO COMPLETO' logs/tr33_corrida.log; do sleep 120; done
echo "=== fechou — $(date +%H:%M)" > analise_33.log
python analisa_33.py >> analise_33.log 2>&1
