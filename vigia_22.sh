#!/usr/bin/env bash
# Espera a guarda fechar as 12 e roda a analise REGISTRADA sozinha.
# `exp_22.sh` escreve "=== CUSTO COMPLETO" no fim; ate la, so espera.
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
while ! grep -aq 'CUSTO COMPLETO' tr22_corrida.log; do sleep 60; done
echo "=== guarda fechou — $(date +%H:%M)" > analise_22.log
python analisa_22.py >> analise_22.log 2>&1
echo "=== FIM — $(date +%H:%M)" >> analise_22.log
