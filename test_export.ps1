Remove-Item -Recurse -Force test_export -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path test_export | Out-Null
Set-Location test_export

Write-Host "`n--- Test 4: Massive Payload (20 separate files) ---"
# Expected limits based on our safety guards
$chunkLimit = 15
$payloadLimit = 3000000
for ($i = 1; $i -le 20; $i++) {
    $content = "C" * 200000
    $content | Set-Content "massive_$i.txt"
}
centaur --export
Write-Host "Checking if any files were generated in temp (Should be empty if test passes):"
$tempPath = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), "centaur_export")
Get-ChildItem $tempPath -ErrorAction SilentlyContinue | Select-Object Name

Set-Location ..
