@echo off
rem Windows shortcut: stm32lab detect | deps | build [A|B|all] | flash A|B | smoke-test [A|B|all]
python "%~dp0stm32lab.py" %*
