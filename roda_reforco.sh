#!/usr/bin/env bash
# Pega a mensagem INTEIRA dos dois testes de reforco que cairam.
# Espera o `modelo` sair: dois cargo disputam o lock do target.
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
while [ ! -f modelo.log ] || ! grep -q '=== FIM' modelo.log; do sleep 15; done
cargo test --release --test reforco -- --nocapture > reforco.log 2>&1
echo "=== FIM codigo=$? — $(date +%H:%M)" >> reforco.log
