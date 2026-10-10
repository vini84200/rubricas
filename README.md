# Rubricas

Avaliação por rubricas: um servidor em Rust que lê e grava uma pasta de arquivos e serve uma
aplicação web (SPA, instalável como PWA) para montar rubricas, atrelá-las a uma turma e avaliar
aluno por aluno, um critério por vez.

## Começar

```sh
cargo build --release
cp -r exemplos dados                                  # opcional: duas rubricas e uma turma fictícia
./target/release/rubricas usuario vini --dados dados  # pede a senha (mínimo de 8 caracteres)
./target/release/rubricas servir --dados dados        # http://127.0.0.1:8080
```

O binário é único: a aplicação web vai embutida nele.

| Comando | O que faz |
| --- | --- |
| `rubricas servir [--dados DIR] [--endereco IP:PORTA] [--web DIR]` | Sobe o servidor. `--web` serve a pasta `web/` do disco em vez da embutida, para desenvolver sem recompilar. |
| `rubricas usuario NOME [--dados DIR]` | Cria o usuário ou troca a senha. Com `RUBRICAS_SENHA` no ambiente, não pergunta. |
| `rubricas usuario NOME --remover` | Remove o usuário; as sessões dele deixam de valer na hora. |
| `rubricas token NOME [--remover]` | Cria o token de API do usuário (substitui o anterior) ou o remove. Veja [Claude e scripts](#claude-e-scripts). |
| `rubricas skill instalar` | Copia a skill do Claude Code para `~/.claude/skills/rubricas/`. |
| `rubricas executor --servidor URL [--continuo]` | Roda as checagens pendentes em contêineres podman. Veja [EAD](#ead-submissões-do-moodle-e-checagens). |

## A pasta de dados

```
dados/
  rubricas/<id>.toml              uma rubrica por arquivo, editável à mão
  turmas/<nome>.csv               lista de alunos
  avaliacoes/<id>.json            título, turma e uma cópia das rubricas usadas
  correcoes/<avaliacao>/<aluno>.json
  submissoes/<avaliacao>/<aluno>/ arquivos enviados pelo aluno (do ZIP do Moodle)
  submissoes/<avaliacao>/.edicoes/<aluno>/  mudanças do professor sobre esses arquivos
  apelidos/<turma>.json           nome no Moodle → aluno, das associações feitas à mão
  pipelines/<avaliacao>/          pipeline.toml e anexos (testes.py, esqueleto.py, referencias/)
  relatorios/<avaliacao>/         resultado das checagens por aluno e do JPlag
  usuarios.toml                   usuários, hashes Argon2id das senhas e SHA-256 dos tokens
  .sessoes.json                   sessões abertas (só o hash de cada token)
```

Tudo é lido do disco a cada pedido: dá para editar, versionar ou sincronizar a pasta com o
servidor no ar. As gravações são atômicas (arquivo temporário + renomear).

### Rubrica (`rubricas/*.toml`)

```toml
titulo = "Trabalho final: código"

[[criterios]]
tipo = "roteiro"                                      # passo de roteiro: orienta, não vale nota
nome = "Rode com entrada vazia"
descricao = "Execute `python main.py` e aperte Enter. Deve pedir de novo."

[[criterios]]
nome = "Compila sem erros"
escala = { tipo = "binaria", pontos = 1.0 }          # atende ou não atende

[[criterios]]
nome = "Modularização"
descricao = "Aparece no cartão durante a avaliação."
peso = 2.0                                            # opcional; multiplica os pontos

[criterios.escala]
tipo = "niveis"                                       # níveis com descritor e pontos

[[criterios.escala.niveis]]
rotulo = "Excelente"
descritor = "Funções curtas e bem nomeadas."
pontos = 2.0

[[criterios.escala.niveis]]
rotulo = "Ausente"
pontos = 0.0

[[criterios]]
nome = "Legibilidade"
escala = { tipo = "livre", max = 2.0, passo = 0.25 }  # nota de 0 a max

[[criterios]]
nome = "Structs e arquivos"
escala = { tipo = "ponto_unico", esperado = "Estado em structs; salva em arquivo.", max = 2.0 }
```

**Passos de roteiro** (`tipo = "roteiro"`) ficam intercalados com os critérios, na ordem do
arquivo: são as instruções a seguir durante a correção (o que rodar, que entrada testar). Não têm
`escala` nem `peso`, não entram na nota, no CSV nem na contagem de critérios. A `descricao` de
passos e critérios aceita Markdown simples: `código`, blocos ``` (com botão de copiar), listas,
**negrito** e *itálico*.

Os campos `id` de critérios e níveis são opcionais em arquivos escritos à mão (saem do nome); o
editor web os grava explicitamente, para que renomear um critério não mude sua identidade. Um
arquivo inválido não derruba nada: aparece como aviso na lista de rubricas.

### Turma (`turmas/*.csv`)

`matricula,nome`, uma linha por aluno. A leitura aceita `,`, `;` ou tabulação, com ou sem
cabeçalho, colunas em qualquer ordem (`nome`, `matricula`, `cartao`, `id`…) e a exportação de
participantes do Moodle (Nome, Sobrenome, Número de identificação). Arquivos na pasta ou
carregados pela tela podem estar em UTF-8 ou Latin-1.

Sem coluna de matrícula, o id sai do nome. Sem cabeçalho, a coluna que parece matrícula (tem
dígitos e não tem espaços) é a matrícula e a outra é o nome; se nenhuma parece, a primeira coluna
é o nome.

### Avaliação e nota

Uma avaliação junta uma turma e uma ou mais rubricas, cada uma com um peso. Ela guarda uma
**cópia** das rubricas: editar a rubrica depois não altera correções feitas, e a tela de notas
avisa quando a cópia ficou diferente do arquivo e oferece atualizá-la.

```
nota = nota_maxima × Σ peso_rubrica × (pontos ÷ máximo da rubrica) ÷ Σ peso_rubrica
pontos da rubrica = Σ peso_critério × pontos do critério
```

Critério sem marcação conta zero; passos de roteiro não contam. A nota é mostrada com duas casas, arredondando meio para cima.
A conta e o arredondamento existem em dois lugares que precisam andar juntos: `src/modelo.rs` e
`src/api.rs` (exportação), `web/js/nota.js` e `web/js/util.js` (tela).

## Avaliar

Um cartão por critério. Cada marcação já avança para o próximo critério ainda sem marcação;
quando não há mais, vem o resumo do aluno e o botão para o próximo aluno que ainda falta. Por
alguns segundos depois de marcar, um botão "Desfazer" devolve o critério ao que era.

| Gesto no cartão | Teclado | Efeito |
| --- | --- | --- |
| deslizar à direita | `→` ou `D` | nível máximo / atende |
| deslizar à esquerda | `←` ou `A` | nível mínimo / não atende |
| deslizar para cima, ou tocar | `↑` | opções: níveis intermediários, nota, comentário |
| deslizar para baixo | `↓` ou `Backspace` | voltar um critério |
| | `1`–`9` | escolher o nível (1 = o de menos pontos) |
| | `S` | pular sem marcar (no celular: "Pular sem marcar", nas opções) |
| | `C` | comentar |
| | `Z` | desfazer a última marcação |
| | `Enter` | no resumo: próximo aluno |
| | `[` `]` | aluno anterior / seguinte |

**Roteiro.** Um passo de roteiro tem cartão próprio, sem marcação: deslizar para qualquer lado,
`→`, `←`, `Enter` ou o botão "Seguir" passam ao item seguinte, e o texto rola dentro do cartão.
Depois de marcar, a avaliação para no roteiro do próximo critério sem marcação, e pula os passos
de critérios já marcados. Quando se chega a um critério sem passar pelo roteiro dele (pela trilha,
pelo resumo, ao voltar ou ao desfazer), os nomes dos passos aparecem resumidos no topo do cartão.
Tocar no cartão do passo também segue, e `Z` desfaz a última marcação mesmo com o passo na tela.

Em tela larga a lista de alunos e as opções do critério ficam sempre visíveis. Os três botões
abaixo do cartão fazem o mesmo que os gestos.

**Sem conexão.** Cada marcação entra numa fila no aparelho antes de ir ao servidor e é reenviada
quando a rede volta; o canto superior mostra "Salvo", "Salvando…" ou quantas faltam enviar. Uma
avaliação já aberta continua utilizável sem rede, inclusive depois de recarregar. O envio é por
critério, então dois avaliadores no mesmo aluno não apagam as marcações um do outro (no mesmo
critério, vale a última). A fila é separada por usuário. Se a rubrica mudou enquanto havia
marcações na fila, as que ainda cabem são gravadas e as outras aparecem num aviso na tela.

## EAD: submissões do Moodle e checagens

**Submissões.** Na tela da avaliação, "Enviar submissões do Moodle (ZIP)" recebe o arquivo de
"Baixar todas as submissões" da tarefa. Cada pasta `Nome Sobrenome_12345_assignsubmission_file_/`
é associada ao aluno da turma de mesmo nome (sem diferença de acentos ou maiúsculas). Num reenvio,
quem está no ZIP tem a submissão substituída e quem não está fica com a que tinha; um reenvio igual
não conta como mudança. Se a submissão muda depois de o aluno ter marcações, o cartão mostra "A
submissão mudou depois da correção", sem apagar nada, até a correção ser mexida de novo. Nomes que
não casam ficam "sem dono" até serem associados na tela; a associação vale para os próximos ZIPs da
turma.

**Código.** Em tela larga (a partir de 1280 px) o código do aluno fica ao lado do cartão, com
números de linha e destaque de Python (Prism, em `web/vendor/prism.js`); em telas menores, o botão
"Código" abre o mesmo painel. A tela **Acompanhar** (`#/acompanhar`), aberta no monitor, mostra o
código e as checagens do aluno que o mesmo usuário abriu em outro aparelho e troca sozinha (SSE,
com batimento a cada 30 s para o proxy não cortar a conexão).

**Editar o código.** O botão "Editar" no código abre um editor (Tab indenta, Enter mantém o
recuo, Ctrl+S salva) para mudanças pontuais, como corrigir um erro de sintaxe e ver se o resto
funciona. Salvar põe o aluno de volta na fila do executor; as checagens passam a usar a versão
editada e o cartão avisa "Código editado por …". O original fica guardado em
`submissoes/<avaliacao>/.edicoes/` à parte: "Ver original" mostra o que o aluno enviou e "Desfazer
edição" volta a ele. O plágio sempre compara os originais. Se um ZIP novo traz código diferente do
aluno, a edição feita sobre o antigo é descartada e a tela avisa.

**Checagens.** O pipeline da avaliação fica em `pipelines/<avaliacao>/pipeline.toml` e é escrito
pela API em texto (a skill do Claude ensina o formato); a tela "Checagens e plágio" só o mostra.
Tipos: `casos` (stdin → stdout; os prompts do `input()` não entram na saída comparada, porque
cada aluno escreve o seu, a não ser com `considerar_prompts = true`), `pytest`, `estilo` (ruff), `comando`, e `plagio` (JPlag) na raiz
do arquivo. Uma checagem pode apontar um critério; o resultado vira uma **sugestão** no cartão
("Sugestão: Atende (8/8 casos)"), que `G` ou um toque aceitam. Código do aluno que não roda conta
como reprovado e o relatório mostra o fim do stderr; falha do executor (imagem, podman) aparece
como "erro de execução", não reprova e o aluno continua pendente.

**Executor.** O servidor nunca executa código de aluno. Quem executa é

```sh
RUBRICAS_TOKEN=… rubricas executor --servidor https://rubrica.exemplo.org            # esvazia a fila e termina
RUBRICAS_TOKEN=… rubricas executor --servidor https://rubrica.exemplo.org --continuo # fica ouvindo
```

numa máquina com podman (o notebook ou a própria VPS). Quando muda a forma de julgar (a constante
`MOTOR` em `src/fila.rs`), todos os trabalhos voltam para a fila uma vez. Cada checagem roda em `podman run
--network=none`, com 256 MB, 1 CPU e 10 s (por caso, nos `casos`; configuráveis no pipeline), numa imagem
derivada do `python:3.12-slim` com pytest e ruff, construída na primeira vez. O plágio roda o
[JPlag](https://github.com/jplag/JPlag) 6.3.0 (GPL-3.0) em outro contêiner (Java 25), como
processo separado: o servidor recebe só os pares e o `.jplag` para o visualizador oficial. A tela
da avaliação mostra quantas submissões aguardam o executor e quando ele foi visto. Com
`--continuo`, ele acorda quando um ZIP chega ou o pipeline é gravado pela API; um `pipeline.toml`
editado direto na pasta é notado em até 10 minutos.

## Claude e scripts

Além da sessão por cookie, a API aceita `Authorization: Bearer <token>`. O token é criado no
servidor com `rubricas token NOME`, mostrado uma vez só e guardado como SHA-256. Ele tem os mesmos
poderes do usuário e deixa de valer quando o usuário troca a senha ou é removido.
**Notas de volta ao Moodle.** Na tarefa, com o feedback "Planilha de notas offline" ligado, baixe
"Planilha de notas" e mande o arquivo em "Preencher planilha do Moodle" na tela da avaliação: a
mesma planilha volta preenchida, pronta para "Carregar planilha de notas" na tarefa. Cada linha
casa pelo "Número de identificação" (se a planilha o traz), pelas associações feitas no ZIP ou
pelo nome. Só vai nota de correção concluída; os comentários dos critérios e o geral vão para
"Comentários de feedback", se a coluna existir e houver algum. O resto da linha volta como veio:
quem não foi avaliado ou está incompleto mantém o que o Moodle tinha, e linha com "A nota pode ser
alterada: Não" não é mexida. O separador decimal segue o da planilha (o Moodle em inglês leria
"7,5" como 75), a planilha sai sempre separada por vírgula (o padrão do envio), e se a nota máxima da tarefa difere da avaliação a nota é convertida. A tela lista
quem ficou de fora e por quê. Código em `src/moodle.rs`.


Para gastar poucos tokens, as rubricas também são lidas e gravadas em texto:

| Pedido | Resposta |
| --- | --- |
| `GET /api/rubricas.txt` | uma linha por rubrica: `id \| título \| N critérios \| M passos` |
| `GET /api/rubricas/{id}.toml` | o arquivo, sem os `id` que saem dos nomes |
| `PUT /api/rubricas/{id}.toml` | valida, grava (ou cria) e responde numa linha; erro com linha e coluna |
| `GET /api/avaliacoes.txt` | uma linha por avaliação: `id \| título \| turma \| rubricas \| com/sem pipeline` |
| `GET /api/pipelines.txt` | uma linha por pipeline: `id \| checagens \| anexos \| pendentes` |
| `GET/PUT /api/pipelines/{avaliacao}/pipeline.toml` | o pipeline; o `PUT` valida contra a avaliação e avisa anexos que faltam |
| `GET/PUT/DELETE /api/pipelines/{avaliacao}/{arquivo}` | anexos do pipeline |

No `PUT`, critérios e níveis sem `id` herdam o id do item de mesmo nome na versão gravada: reescrever
o arquivo inteiro não muda identidades. A skill em `claude/skills/rubricas/SKILL.md` ensina isso ao
Claude Code. Instale com `rubricas skill instalar` e defina `RUBRICAS_URL` e `RUBRICAS_TOKEN` no
ambiente da máquina onde o Claude roda.

## Autenticação e publicação

- Usuários e tokens são criados só pela linha de comando; não há cadastro pela web.
- Sessão por cookie `HttpOnly`, `SameSite=Strict`, 30 dias; ganha `Secure` quando o pedido chega
  com `X-Forwarded-Proto: https`.
- Trocar a senha, remover ou recriar um usuário encerra as sessões abertas dele.
- "Sair" sem conexão não consegue encerrar a sessão no servidor na hora: o aparelho volta à tela
  de entrada e encerra a sessão assim que a rede voltar.
- Oito senhas erradas para o mesmo usuário bloqueiam novas tentativas por até 10 minutos.
- Todos os usuários veem e editam tudo: não há papéis nem separação por turma.

O padrão é escutar só em `127.0.0.1`. Para usar no celular, publique atrás de um proxy com HTTPS
(navegadores só instalam PWA e ativam o modo offline em HTTPS ou em `localhost`). Com Caddy:

```
rubricas.exemplo.org {
    reverse_proxy 127.0.0.1:8080
}
```

Com Docker, há um `Dockerfile` (binário sobre `distroless`) e um `compose.yaml` de exemplo que
publica só em `127.0.0.1:8080`, para o proxy HTTPS ficar na frente:

```sh
docker compose up -d --build
docker compose run --rm rubricas usuario vini --dados /dados
docker compose run --rm rubricas token vini --dados /dados
```

## Desenvolver

```sh
cargo test                                             # modelo, pasta de dados, autenticação, ZIP, pipeline, fila, planilha do Moodle
cargo run -- servir --dados dados --web web            # front-end do disco, sem recompilar
```

O front-end não tem etapa de build: módulos ES em `web/js`, com `htm` + Preact num arquivo só em
`web/vendor/preact-htm.js` (cópia de `htm@3.1.1`, `preact/standalone.module.js`), o Prism em
`web/vendor/prism.js` (`prismjs@1.30.0`, core e Python, embrulhados como módulo ES) e a fonte
Atkinson Hyperlegible em `web/fontes` (de `@fontsource/atkinson-hyperlegible@5.3.0`).

## O que não faz

- Avaliar em dupla ou grupo: a correção é por aluno.
- Conversar com o Moodle: as submissões entram pelo ZIP baixado à mão e as notas saem pela
  planilha de notas da tarefa (ou em CSV, um normal e um para o Excel em português).
- Comentários por linha no código: o comentário continua sendo por critério.
- Edição simultânea da mesma rubrica: a última gravação vence, sem aviso.
- Histórico de alterações das correções: o arquivo guarda só o estado atual, quem alterou por
  último e quando.
