#!/usr/bin/env bash
# EXPERIMENTO: variedade de VERBO conserta a abstencao indevida?
#
# MEDICAO QUE MOTIVA (12 sementes, 2026-09-05):
#   "resgata um arquivo chamado chave" e 0,700 parecida com o molde
#   "procura um arquivo chamado {0}" — e mesmo assim ela ABSTEM em 8 de 12.
#   A unica diferenca e o verbo. Ela nao generalizou sobre verbo.
#
# MUDANCA: +62 moldes, so variedade de verbo e de queixa figurada, em
#   procurar_arquivo, hora, memoria, disco, ler_arquivo, listar_pasta.
#   Nenhum verbo do benchmark nem das reguas do John e do amigo.
#
# LINHA DE BASE: o braco `f66` (112,75 +- 3,11), NAO o `pp` (110,67).
#   f66 tem os 66 fora-de-escopo, que ja estao no codigo. Comparar contra pp
#   misturaria dois efeitos.
#
# CONFUNDIDOR JA CONHECIDO, e como descontar: entre f66 e hoje eu tambem TIREI
#   a regua vazada "e ai como voce ta hoje" do poco. Ela sozinha vale ~0,83
#   ponto do benchmark e ~0,83 falha por semente na sonda. Entao a comparacao
#   limpa EXCLUI essa frase dos dois lados:
#
#       sonda dirigida, 27 frases (sem a vazada):  f66 = 10,00 falhas/semente
#
# INSTRUMENTO QUE DECIDE: a sonda dirigida de abstencao. sd medido 2,61,
#   contra 4,48 do benchmark de 150 — que serve so para confirmar que nada
#   quebrou em volta.
#
# n FIXADO EM 12. Nao ler parcial.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
for s in 7 8 9 10 11 12 13 14 15 16 17 18; do
    echo "=== verbos semente ${s} — $(date +%H:%M) ==="
    ./teka_exp.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
        --semente "$s" --threads 10 --saida "modelos/teka_vb_s${s}.bin" --benchmark \
        > "logs/vb_s${s}.log" 2>&1
    grep -aE 'ferramenta certa:|argumento quando' "logs/vb_s${s}.log"
done
