#!/usr/bin/env bash
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
M=$(ls -t modelos/teka_tres_s*.bin | head -1)
./target/release/teka.exe agente --carregar "$M" < /tmp/carga.txt > /tmp/folego_saida.txt 2>&1
echo "=== FIM $(date +%H:%M:%S)" >> /tmp/folego_saida.txt
