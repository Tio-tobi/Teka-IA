#!/usr/bin/env bash
# Os dois consertos, contra os testes que os motivaram e contra os que eles podem
# ter quebrado.
#
#   lib        as duas cabecas mexeram em `heads.rs`, que a lib inteira usa
#   gradcheck  cabeca nova = gradiente novo; e eu passei a ZERAR `dvalor`, que
#              antes ficava com sobra do lote anterior quando ninguem escrevia
#   memoria    o teste que a mudanca da assinatura tem de consertar
#   reforco    os dois que a separacao das cabecas tem de consertar
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
: > verifica.log
for t in "--lib" "--test gradcheck" "--test memoria" "--test reforco"; do
  echo "### cargo test --release $t — $(date +%H:%M)" >> verifica.log
  cargo test --release $t -- --nocapture >> verifica.log 2>&1
  echo "### codigo=$? — $(date +%H:%M)" >> verifica.log
done
echo "### FIM — $(date +%H:%M)" >> verifica.log
