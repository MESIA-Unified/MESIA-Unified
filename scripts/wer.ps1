$e = Get-WinEvent -FilterHashtable @{LogName='System'; ProviderName='nvlddmkm'; StartTime=(Get-Date).AddHours(-4)} -MaxEvents 4 -ErrorAction SilentlyContinue
foreach ($ev in $e) {
  $x = [xml]$ev.ToXml()
  $d = foreach ($item in $x.Event.EventData.Data) { "$($item.Name)=$($item.InnerText)" }
  $ev.TimeCreated.ToString('HH:mm:ss') + ' :: ' + ($d -join '; ')
}
