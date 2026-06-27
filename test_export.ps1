Remove-Item -Recurse -Force test_export -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path test_export | Out-Null
Set-Location test_export

Write-Host "`n--- Test 4: Massive Payload (15 separate files) ---"
for ($i = 1; $i -le 15; $i++) {
    $content = "C" * 110000
    $content | Set-Content "massive_$i.txt"
}
centaur --export
Write-Host "Checking if any files were generated (Should be empty):"
Get-ChildItem centaur_context_part*.txt -ErrorAction SilentlyContinue | Select-Object Name

Set-Location ..
