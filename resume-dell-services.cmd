@echo off
powershell -Command "Start-Process powershell -Verb RunAs -ArgumentList '-ExecutionPolicy Bypass -NoExit -File \"%~dp0resume-dell-services.ps1\"'"
