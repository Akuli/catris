reg add HKCU\Software\SimonTatham\PuTTY\Sessions\CATRIS /f /v HostName /d 172.104.132.97
reg add HKCU\Software\SimonTatham\PuTTY\Sessions\CATRIS /f /v PortNumber /t REG_DWORD /d 12345
reg add HKCU\Software\SimonTatham\PuTTY\Sessions\CATRIS /f /v Protocol /d raw

reg add HKCU\Software\SimonTatham\PuTTY\Sessions\CATRIS /f /v LocalEcho /t REG_DWORD /d 1
reg add HKCU\Software\SimonTatham\PuTTY\Sessions\CATRIS /f /v LocalEdit /t REG_DWORD /d 1

reg add HKCU\Software\SimonTatham\PuTTY\Sessions\CATRIS /f /v TermWidth /t REG_DWORD /d 80
reg add HKCU\Software\SimonTatham\PuTTY\Sessions\CATRIS /f /v TermHeight /t REG_DWORD /d 32

reg add HKCU\Software\SimonTatham\PuTTY\Sessions\CATRIS /f /v Font /d Consolas
reg add HKCU\Software\SimonTatham\PuTTY\Sessions\CATRIS /f /v FontHeight /t REG_DWORD /d 14
reg add HKCU\Software\SimonTatham\PuTTY\Sessions\CATRIS /f /v FontIsBold /t REG_DWORD /d 0
reg add HKCU\Software\SimonTatham\PuTTY\Sessions\CATRIS /f /v FontCharSet /t REG_DWORD /d 0

powershell -Command "(New-Object System.Net.WebClient).DownloadFile('https://the.earth.li/~sgtatham/putty/0.76/w64/putty.exe', 'catris-putty.exe')"

certutil -hashfile catris-putty.exe SHA256 | find "05 81 16 09 98 be 30 f7 9b d9 a0 92 5a 01 b0 eb c4 cb 94 26 5d fa 7f 8d a1 e2 83 9b f0 f1 e4 26"
if errorlevel 1 (exit /b 1)

start /b catris-putty.exe -load CATRIS
