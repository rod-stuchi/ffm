# FFM — português do Brasil.
#
# Stub: every id from en.ftl is present with the same placeables, which is what
# build.rs checks. Values are being translated incrementally — an id still
# carrying its English text is visibly untranslated in the sandbox's
# side-by-side view (/__sandbox/rows?lang=both) rather than silently missing.
#
# Household sample data — "Água · Sabesp", "Mesada" — is NOT app copy and must
# never appear in this file. It is content the household typed.

## Pré-login

login-title = Entrar
login-name = Nome
login-password = Senha
login-submit = Entrar
login-error-mismatch = Nome e senha não conferem.

setup-title = Configure sua casa
setup-household-section = Sua casa
setup-you-section = Você
setup-storage-section = Definido pelo operador
setup-household-name = Nome da casa
setup-currency = Moeda
setup-language = Idioma padrão
setup-password-confirm = Confirme a senha
setup-submit = Criar casa
setup-taken-title = Esta casa já foi configurada
setup-taken-body = Alguém já concluiu a configuração. Entre em vez disso.

invite-title = Entrar em { $household }
invite-name-taken = Esse nome já está em uso — escolha outro.
invite-dead-title = Este link expirou
invite-dead-body = Peça um novo para um administrador.

## Painel

ledger-ins = ENTRADAS
ledger-outs = SAÍDAS
ledger-balance = Saldo
ledger-household-in = Entradas da casa
ledger-household-out = Saídas
ledger-household-net = Líquido

## Estados da linha

row-overdue-since = vencido desde { $date }
row-scheduled-for = agendado · vence { $date }
row-due = vence { $date }
row-paid-on = pago em { $date }
row-received-on = recebido em { $date }
row-manual-paid = marcado como pago
row-deleted = excluído
row-unsaved = não salvo · e para retomar

## Arquivos

file-bill = Conta
file-invoice = Fatura
file-scheduled = Agendado
file-receipt = Comprovante
file-payslip = Holerite
file-page = página { $page }
file-none-bill = nenhuma conta neste lançamento
file-none-scheduled = nenhum agendamento neste lançamento
file-none-receipt = nenhum comprovante neste lançamento
file-none-income-scheduled = agendamento não se aplica a uma entrada

## Recusas na statusline

sort-refuses-reorder = ledger.sort = "{ $sort }" — reordenar exige sort = "manual"
group-refuses-reorder = ledger.group = "{ $group }" — reordenar exige group = "none"
not-a-command = não é um comando: { $name } (tente help)
no-rule-behind-row = não foi gerado por uma regra recorrente

rows-moved =
    { $n ->
        [one] 1 linha movida { $dir }
       *[other] { $n } linhas movidas { $dir }
    }
