# Quem pode fazer o quê. Avaliada pelo regorus (Rego, a linguagem do Open Policy Agent).
#
# Para adaptar: `rubricas politica > dados/politica.rego`, edite, reinicie o servidor.
# Um arquivo com erro impede o servidor de subir, em vez de liberar ou negar tudo às cegas.
#
# Entrada (`input`):
#   usuario.nome, usuario.papel   papel global: admin | professor | monitor | executor
#   recurso.tipo, recurso.id      tipo: sistema | avaliacao | rubrica | turma
#   acesso                        papel do usuário neste recurso (concedido a ele), ou null
#
# Saída: `acoes`, o conjunto de ações permitidas ao usuário sobre o recurso.
package rubricas

import rego.v1

# Papéis que podem ser concedidos em cada tipo de recurso, do menor para o maior.
# A tela de compartilhar mostra estes, nesta ordem.
papeis := {
	"avaliacao": ["leitor", "avaliador", "editor", "dono"],
	"rubrica": ["leitor", "editor", "dono"],
	"turma": ["leitor", "editor", "dono"],
}

papeis_globais := ["admin", "professor", "monitor", "executor"]

# Papel de quem é criado pelo terminal sem --papel (o primeiro usuário recebe
# um papel que pode gerir_usuarios). Opcional: sem ele, o terminal pede o papel.
papel_global_padrao := "monitor"

# O que cada papel acrescenta aos anteriores (na ordem de `papeis`); cada um
# pode tudo o que os de antes podem.
#   ver            abrir, listar, ler arquivos e relatórios
#   exportar       baixar as notas (CSV, planilha do Moodle)
#   corrigir       marcar a rubrica e editar o código do aluno
#   editar         mudar a avaliação, importar submissões, mudar as checagens
#   arquivar, apagar, compartilhar
#   ler_checagens  baixar pipeline e código para rodar as checagens (o executor)
# O último papel de cada lista é o de quem cria o item, e todo item precisa
# de pelo menos um.
acrescenta := {
	"avaliacao": {
		"leitor": {"ver", "exportar", "ler_checagens"},
		"avaliador": {"corrigir"},
		"editor": {"editar", "arquivar"},
		"dono": {"apagar", "compartilhar"},
	},
	"rubrica": {
		"leitor": {"ver"},
		"editor": {"editar", "arquivar"},
		"dono": {"apagar", "compartilhar"},
	},
	"turma": {
		"leitor": {"ver"},
		"editor": {"editar", "arquivar"},
		"dono": {"apagar", "compartilhar"},
	},
}

acoes_do_papel := {tipo: por_papel |
	some tipo, lista in papeis
	por_papel := {papel: acoes |
		some i, papel in lista
		acoes := {a | some anterior in array.slice(lista, 0, i + 1); some a in acrescenta[tipo][anterior]}
	}
}

# Ações sobre o sistema como um todo.
#   criar           criar rubricas, turmas e avaliações (quem cria vira dono)
#   gerir_usuarios  criar, remover e mudar o papel de usuários
#   executar        pegar tarefas da fila de checagens e enviar resultados
#   ver_tudo        enxergar e mexer em tudo, mesmo sem acesso concedido
# Ao menos um usuário precisa poder gerir_usuarios.
acoes_globais := {
	"admin": {"criar", "gerir_usuarios", "executar", "ver_tudo"},
	"professor": {"criar"},
	"monitor": set(),
	"executor": {"executar"},
}

default acoes := set()

acoes := acoes_globais[input.usuario.papel] if input.recurso.tipo == "sistema"

acoes := todas if {
	input.recurso.tipo != "sistema"
	ve_tudo
}

acoes := concedidas | do_executor if {
	input.recurso.tipo != "sistema"
	not ve_tudo
}

ve_tudo if "ver_tudo" in acoes_globais[input.usuario.papel]

maior := papeis[input.recurso.tipo][count(papeis[input.recurso.tipo]) - 1]

todas := acoes_do_papel[input.recurso.tipo][maior]

concedidas := acoes_do_papel[input.recurso.tipo][input.acesso] if input.acesso != null

default concedidas := set()

# O executor baixa o que precisa para rodar as checagens de qualquer avaliação,
# mas não vê notas nem correções.
do_executor := {"ler_checagens"} if {
	input.usuario.papel == "executor"
	input.recurso.tipo == "avaliacao"
}

default do_executor := set()
