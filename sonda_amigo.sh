#!/usr/bin/env bash
# Roda o teste do amigo contra um modelo. Le dados/teste_do_amigo.txt.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
BIN=${BIN:-./teka_exp.exe}
MODELO=${1:?uso: sonda_amigo.sh <modelo.bin>}
shift || true
ok_f=0; ok_a=0; n=0
while IFS='|' read -r ferr frase arg; do
    case "$ferr" in \#*|'') continue ;; esac
    ferr=$(echo "$ferr" | xargs); frase=$(echo "$frase" | sed 's/^ *//;s/ *$//'); arg=$(echo "$arg" | sed 's/^ *//;s/ *$//')
    [ -z "$frase" ] && continue
    n=$((n+1))
    saida=$($BIN agente --carregar "$MODELO" "$@" --pedido "$frase" 2>&1)
    acao=$(echo "$saida" | grep -aoE '"acao":"[a-z_]+"' | head -1 | sed 's/.*:"//;s/"//')
    valor=$(echo "$saida" | grep -aoE '"consulta":"[^"]*"' | head -1 | sed 's/^"consulta":"//;s/"$//')
    [ "$acao" = "$ferr" ] && ok_f=$((ok_f+1))
    [ "$valor" = "$arg" ] && ok_a=$((ok_a+1))
    printf "  %-22s %s\n" "${acao:-(nada)}" "${valor:0:46}"
done < dados/teste_do_amigo.txt
echo "  --- $(basename "$MODELO"): ferramenta ${ok_f}/${n} | argumento exato ${ok_a}/${n}"
