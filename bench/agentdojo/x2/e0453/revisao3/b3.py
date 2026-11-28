from gen import mk
X = '''tool send(c: Cap[Bank.Pay], to: Text ^ Endorsed, memo: Text) -> Bool ! { Bank.Pay } reversible latency 1.0
tool undo(c: Cap[Bank.Pay], to: Text ^ Endorsed) -> Unit ! { Bank.Pay } reversible latency 1.0
tool ok(c: Cap[Bank.Pay], to: Text ^ Endorsed) -> Bool ! { Bank.Pay } reversible latency 1.0'''
mk('b15_await_mix', '''      for a in ops { for b in ops {
        let r = atomic recover ok(bank, a.to) {
          pay(bank, a.to, a.memo)
          await send(bank, a.to, b.memo) within 1.0
          "feito"
        } on failure(e) { "falhou" }
        pay(bank, a.to, r)
      } }''', X)
mk('b15b_ctrl', '''      for a in ops { for b in ops {
        let r = atomic recover ok(bank, a.to) {
          pay(bank, a.to, a.memo)
          send(bank, a.to, b.memo)
          "feito"
        } on failure(e) { "falhou" }
        pay(bank, a.to, r)
      } }''', X)
