#!/usr/bin/env bash
# Espera as DUAS corridas e roda as duas analises.
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
while ! grep -aq 'PASTA COMPLETA' logs/pasta_corrida.log 2>/dev/null; do sleep 120; done
echo "=== destino de pasta fechou — $(date +%H:%M)" > logs/analise_pasta.log
python analisa_pasta.py >> logs/analise_pasta.log 2>&1
while ! grep -aq 'SEM IMAGEM COMPLETA' logs/semimg_corrida.log 2>/dev/null; do sleep 120; done
echo "=== sem ler_imagem fechou — $(date +%H:%M)" > logs/analise_semimg.log
python analisa_semimg.py >> logs/analise_semimg.log 2>&1
echo "AS DUAS FECHARAM"
