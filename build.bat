@echo off

if not exist build mkdir build
call rustc -o build\calendar-fast.exe %* main.rs
