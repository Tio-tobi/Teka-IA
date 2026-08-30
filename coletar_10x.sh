#!/usr/bin/env bash
# Coleta encadeada ate o corpus da Teka chegar a 10x.
#
# Roda em rodadas porque a API estrangula: uma execucao unica pedindo tudo levou 437
# recusas em 560 pedidos. Cada rodada grava incremental; a seguinte pula pela retomada
# o que ja esta em disco. Cair no meio nao perde nada.
#
# Tres decisoes aqui foram medidas, nao supostas:
#
#  1. Modo INTRODUCAO, nao artigo inteiro. 40 titulos de "Algoritmos" renderam 17.852
#     bytes por pedido em lote de 20, contra 5.376 pedindo artigo inteiro um por vez.
#     O limite do servidor e por PEDIDO, entao e essa razao que manda.
#
#  2. ENUMERACAO como fonte principal de titulo. Categoria devolve os mesmos titulos
#     toda rodada e a retomada os pula — a segunda rodada renderia zero. Sorteio nao
#     repete mas levou 437 recusas em 560 tentativas. Enumeracao continua do cursor em
#     disco e percorre o acervo sem repetir.
#
#  3. CATEGORIA so na primeira rodada. Rende ~3x mais byte por pedido, mas so tem
#     titulo novo a oferecer uma vez; mante-la nas rodadas seguintes gastaria 60
#     pedidos por rodada para trazer o que a retomada ja descarta.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

SAIDA="dados/corpus_wiki.txt"
ALVO=65000000          # +65 MB sobre os 7,2 MB atuais = 10x
RODADAS=60
PAUSA_ENTRE=180        # respiro entre rodadas, para a franquia do servidor voltar

CATS="Informática,Software,Sistemas operativos,Sistemas de ficheiros,Armazenamento de dados,\
Memória de computador,Interpretadores de comandos,Hardware,Programação,Algoritmos,\
Redes de computadores,Segurança da informação,Bases de dados,Linguagens de programação,\
Internet,Telecomunicações,Eletrónica,Engenharia,Estatística,Matemática,\
Física,Química,Biologia,Astronomia,Geologia,Medicina,Anatomia,Genética,Ecologia,\
Botânica,Zoologia,Filosofia,Psicologia,Sociologia,Antropologia,Linguística,\
Economia,Direito,Política,Educação,História,Geografia,Literatura,Música,Cinema,\
Pintura,Arquitetura,Culinária,Desporto,Religião,Mitologia,Elementos químicos,\
Planetas,Línguas,Doenças,Aves,Mamíferos,Peixes,Escritores,Compositores"

echo "=== coleta 10x — alvo ${ALVO} bytes em ${SAIDA} ==="
for r in $(seq 1 "$RODADAS"); do
    tam=0
    [ -f "$SAIDA" ] && tam=$(wc -c < "$SAIDA")
    if [ "$tam" -ge "$ALVO" ]; then
        echo "=== alvo atingido: ${tam} bytes em ${r} rodadas ==="
        break
    fi
    echo ""
    echo "=== rodada ${r}/${RODADAS} — ${tam} bytes ate agora ==="

    if [ "$r" -eq 1 ]; then
        ./teka_col.exe coletar \
            --categorias "$CATS" --paginas 500 \
            --todas 6000 --pausa 2500 --saida "$SAIDA"
    else
        ./teka_col.exe coletar \
            --categorias "," \
            --todas 6000 --pausa 2500 --saida "$SAIDA"
    fi

    sleep "$PAUSA_ENTRE"
done

tam=$(wc -c < "$SAIDA" 2>/dev/null || echo 0)
echo ""
echo "=== fim: ${tam} bytes, $(grep -c '^# fonte:' "$SAIDA" 2>/dev/null || echo 0) paginas ==="
echo "para juntar no corpus de treino:"
echo "  cat dados/corpus_pt.txt dados/corpus_tec.txt ${SAIDA} > dados/corpus_misto10.txt"
