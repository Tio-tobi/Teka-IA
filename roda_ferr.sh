#!/usr/bin/env bash
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
TEKA_PONTE_TOKEN=teste-manual-1234 ./target/release/examples/sonda_ferramentas.exe > logs/ferr.log 2>&1
echo "=== FIM $(date +%H:%M)" >> logs/ferr.log
