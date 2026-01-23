# Tables

Programmers are familiar with lists, objects, arrays, maps, collections, but business people go hard on tables.

```lemma

type money = scale
  -> unit eur 1.00

type employee_table = table
  -> column id      number
  -> column name    text
  -> column salary  money

fact employess = [employee_table]

rule highest_earning_employee = employees
  -> sort salary desc
  -> limit 1
```