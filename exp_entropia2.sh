#!/usr/bin/env bash
# A pergunta que este experimento responde: patch por entropia melhora a acuracia?
#
# Tudo ate aqui preparou o mecanismo e mostrou que o corpus o alimenta melhor. Nada
# mediu se ele SERVE. Linha de base a bater: 113, 114, 111 (media 112,7 de 150).
#
# Desenho:
#   - mesma execucao, so a flag do patcher muda (recompilar entre bracos meteria o
#     compilador dentro do experimento)
#   - 3 sementes por braco, porque n=1 ja enganou este projeto pelo menos tres vezes
#   - o braco base RE-RODA em vez de reusar os numeros de ontem: o encanamento do
#     patcher virou despacho dinamico no meio, e comparar atraves de versoes de codigo
#     e exatamente como se inventa um efeito que nao existe
#   - n-grama do corpus CONGELADO (corpus_misto10.txt), nao do que a coleta esta
#     engordando agora
#
# SEQUENCIAL de proposito. A versao paralela disto foi o que travou a maquina do John.
# Contagem de thread nao muda resultado (ha teste de igualdade exata contra oraculo
# escalar), so relogio — entao nao ha nada a ganhar rodando tudo de uma vez.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

# Binario com nome PROPRIO, copiado do build. Nao e capricho: coleta e experimento
# rodavam os dois como `teka.exe`, e um `taskkill //IM teka.exe` para reiniciar a
# coleta matou os treinos das sementes 7 e 8 no meio. Nome distinto torna o engano
# impossivel.
BIN=./teka_exp.exe
T=10                      # 12 nucleos, 2 de folga para a coleta que roda junto
NGRAMA=dados/ngrama_o5.bin
LIMIAR=5.5

echo "=== experimento: patch por entropia contra por_palavra ==="
echo "n-grama: ordem 5, 5,5% de posicoes sem dados, 4,130 bytes/patch"
echo "por_palavra: 4,974 bytes/patch — a entropia custa ~20% mais backbone"
echo ""

for braco in base entropia; do
    if [ "$braco" = "base" ]; then
        EXTRA=""
    else
        EXTRA="--patcher entropia --ngrama $NGRAMA --limiar $LIMIAR"
    fi
    for s in 10 11 12; do
        echo "=== ${braco} semente ${s} — $(date +%H:%M) ==="
        $BIN agente --epocas 12 --exemplos 16000 --semente "$s" --threads "$T" \
            $EXTRA --saida "teka_ent_${braco}_s${s}.bin" --benchmark \
            > "ent_${braco}_s${s}.log" 2>&1
        grep -aE 'ferramenta certa|argumento certo|bytes/patch' "ent_${braco}_s${s}.log"
    done
done

echo ""
echo "=== resumo ==="
for braco in base entropia; do
    printf "%-10s" "$braco"
    for s in 10 11 12; do
        grep -a 'ferramenta certa' "ent_${braco}_s${s}.log" 2>/dev/null \
            | sed 's/.*ferramenta certa: /  /;s/ .*//' | tr -d '\n'
    done
    echo ""
done
