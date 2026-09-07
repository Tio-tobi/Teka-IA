#!/usr/bin/env bash
# ===================================================================
# QUANTO CUSTA A FERRAMENTA 20? (`atalho`)
# ===================================================================
#
# A PERGUNTA DO JOHN: "treina com as 20 ferramentas e mede o custo".
#
# Uma ferramenta nova nao e de graca. Ela acrescenta uma classe a cabeca de
# intencao, um poco de argumento novo (75 frases de gatilho) e mais superficie
# para o modelo confundir. A pergunta e se as OUTRAS 19 pioram.
#
# -------------------------------------------------------------------
# METADE DA MEDICAO JA ESTA PAGA
# -------------------------------------------------------------------
#
#   O braco `vb` da corrida de confirmacao (cf_vb_s19..s30, 12 sementes, ja
#   no disco) foi treinado com o catalogo de 19 ferramentas — `teka_vb.exe`
#   e de 41ab886, e `atalho` entrou em 4c02b67, depois.
#
#   E o diff de `dados.rs` entre 41ab886 e HEAD e de 37 linhas, todas dos 4
#   commits do `atalho`. Ou seja: o confundimento e pequeno e conhecido.
#
#   Entao so falta rodar o braco de HEAD, nas MESMAS sementes. 12 corridas,
#   nao 24.
#
# -------------------------------------------------------------------
# INSTRUMENTO, REGISTRADO ANTES
# -------------------------------------------------------------------
#
#   PRIMARIO: `ferramenta certa` sobre as 150 frases, pareado por semente.
#   `dados/frases_teste.txt` NAO mudou entre 41ab886 e HEAD (conferido no
#   git) — a regua e literalmente a mesma nos dois bracos.
#
#   Se a ferramenta 20 custa caro, ESTE numero cai.
#
#   base do braco de 19: 112,9 de 150 (media de s19..s30, desvio 4,4)
#
# -------------------------------------------------------------------
# A MINA DA LINHA 117 — dita ANTES de rodar
# -------------------------------------------------------------------
#
#   A linha 117 do benchmark e:
#
#       perguntar | quero ouvir uma playlist relaxante |
#
#   O gabarito diz `perguntar` porque, quando a frase foi escrita, tocar
#   musica NAO era capacidade dela. Agora e: "quero a playlist" e "quero
#   ouvir" sao gatilhos. O gabarito envelheceu.
#
#   Entao o braco de 20 leva um erro numa frase em que ele esta CERTO. E um
#   ponto de vies SISTEMATICO contra o braco que estou medindo.
#
#   Eu NAO conserto a linha: consertar mudaria a regua entre os bracos, e ai
#   nada seria comparavel. Registro os dois numeros:
#
#     PRIMARIO    150 cruas — conservador, penaliza o braco novo
#     SECUNDARIO  149, sem a 117 — o gabarito honesto de hoje
#
#   Lidero pelo primario de proposito: e o numero que nao me favorece. Se ele
#   nao mostrar custo, o secundario so pode estar melhor.
#
# -------------------------------------------------------------------
# PODER
# -------------------------------------------------------------------
#
#   Desvio pareado das corridas anteriores: ~3,8 pontos. Com n=12, o efeito
#   detectavel a 80% e ~3,2 pontos de 150 (2,1 pontos percentuais).
#
#   ENTAO: isto detecta um custo GRANDE. Um nulo aqui significa "a ferramenta
#   20 nao custou mais que ~2 pontos percentuais", nao "custou zero".
#
#   n FIXADO EM 12. Nao ler parcial, nao estender depois de olhar.
#
#   O braco de 19 nao e re-rodado: ele ja existe, nas mesmas sementes, com a
#   mesma regua. Rodar de novo so gastaria 6 horas para achar outro ruido.
# ===================================================================
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

# RETOMAVEL: so conta como pronta a corrida cujo log chegou ao fim.
pronta() {
  [ -f "ct_20_s$1.log" ] && grep -aq "ferramenta certa:" "ct_20_s$1.log"
}

for s in 19 20 21 22 23 24 25 26 27 28 29 30; do
  if pronta "$s"; then
    echo "=== semente ${s} — ja pronta, pulando ==="
    continue
  fi
  echo "=== 20 ferramentas, semente ${s} — $(date +%H:%M) ==="
  ./teka_20.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
    --semente "$s" --threads 10 --saida "teka_ct_20_s${s}.bin" --benchmark \
    > "ct_20_s${s}.log" 2>&1
done
echo "=== CUSTO COMPLETO — $(date +%H:%M) ==="
