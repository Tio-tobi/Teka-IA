#!/usr/bin/env bash
# Varredura do vies de abstencao, nos dois bracos.
#
# O DESENHO importa mais que a varredura. Escolher o vies que maximiza o benchmark e
# depois reportar o benchmark e ajustar a regua — o projeto ja tem historico de efeito
# que so existia porque a analise parcial foi lida cedo.
#
# Entao a varredura nao serve para escolher o melhor: serve para levantar a curva
# (abstencao, acuracia) dos dois bracos. A comparacao honesta e ler a acuracia dos dois
# NA MESMA taxa de abstencao. Isso remove o confundidor em vez de ajusta-lo a metrica.
#
# O braco de entropia se absteve 23,7 de 150 contra 15,7 do base. A pergunta e se as
# 3 frases que ele perde no agregado sao a hesitacao extra ou o patcher em si.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
BIN=./teka_exp.exe

echo "braco,semente,vies,certas,respondidas,argumento"
for braco in base entropia; do
    if [ "$braco" = "entropia" ]; then
        EXTRA="--patcher entropia --ngrama dados/ngrama_o5.bin --limiar 5.5"
    else
        EXTRA=""
    fi
    for s in 7 8 9; do
        for v in -4.0 -3.5 -3.0 -2.5 -2.0 -1.5 -1.0 -0.5 0.0 0.5 1.0; do
            saida=$($BIN agente --carregar "teka_ent_${braco}_s${s}.bin" $EXTRA \
                     --vies-abster "$v" --benchmark --epocas 0 2>&1)
            certas=$(echo "$saida" | grep -a 'ferramenta certa:' | sed 's#.*: \([0-9]*\)/.*#\1#')
            resp=$(echo "$saida" | grep -a 'entre as que ela responde' | sed 's#.*/\([0-9]*\) .*#\1#')
            arg=$(echo "$saida" | grep -a 'argumento certo' | sed 's#.*: *\([0-9]*\)/.*#\1#')
            echo "${braco},${s},${v},${certas},${resp},${arg}"
        done
    done
done
