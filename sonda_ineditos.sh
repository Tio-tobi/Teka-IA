#!/usr/bin/env bash
# Sonda dirigida: a hipotese ESPECIFICA que o patch por entropia deveria resolver.
#
# O benchmark de 150 frases mede tudo junto e disse "nao ajudou" (-3,0, t = -1,19).
# Mas a hipotese nunca foi "melhora tudo": era que contexto inedito recebe entropia
# alta, ganha fronteira de patch, e com isso a cabeca de PRESENCA volta a enxergar o
# segundo argumento. Se isso vale, aparece aqui e nao no agregado.
#
# Todo nome de arquivo abaixo e inventado, fora do poco de 60 que a geradora usa. O
# sintoma a procurar e o colapso `mover -> apagar`: quando a presenca nao ve o segundo
# argumento, a intencao cai na vizinha de um argumento so.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
BIN=./teka_exp.exe

FRASES=(
  "move zephyr_qx.md para trabalho_final_v3.odt"
  "move dados_brutos_zz.json para tratados_yy.json"
  "move apontamentos_2029.txt para arquivo_morto.md"
  "move relatorio_kx.odt para entregues_wp.txt"
  "move rascunho_jj9.md para publicado_mm.md"
  "renomeia planilha_kx9.csv para resumo_bimestral.ods"
  "renomeia anotacao_vv.txt para memoria_tecnica.md"
  "renomeia captura_zt8.png para diagrama_final.png"
  "renomeia listagem_qp.csv para inventario_novo.csv"
  "renomeia minuta_hh.odt para contrato_assinado.odt"
  "copia esboco_wq.md para versao_definitiva.txt"
  "copia registro_pl3.log para backup_semanal.log"
  "copia modelo_xk.odt para proposta_enviada.odt"
  "copia tabela_rr7.csv para consolidado_ano.csv"
  "copia notas_bz.md para compartilhado_eq.md"
)

for braco in base entropia; do
    if [ "$braco" = "entropia" ]; then
        EXTRA="--patcher entropia --ngrama dados/ngrama_o5.bin --limiar 5.5"
    else
        EXTRA=""
    fi
    for s in 7 8 9; do
        acertos=0; colapsos=0; total=0
        for f in "${FRASES[@]}"; do
            esperado=$(echo "$f" | grep -qE '^copia' && echo copiar_arquivo || echo mover_arquivo)
            r=$($BIN agente --carregar "teka_ent_${braco}_s${s}.bin" $EXTRA \
                  --pedido "$f" 2>&1 | grep -oE '"acao":"[a-z_]+"' | head -1 \
                  | sed 's/.*:"//;s/"//')
            total=$((total+1))
            [ "$r" = "$esperado" ] && acertos=$((acertos+1))
            [ "$r" = "apagar_arquivo" ] && colapsos=$((colapsos+1))
        done
        echo "${braco} s${s}: intencao ${acertos}/${total} | colapsos para apagar: ${colapsos}"
    done
done
