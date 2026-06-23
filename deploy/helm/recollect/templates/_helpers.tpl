{{- define "recollect.name" -}}{{ .Chart.Name }}{{- end -}}
{{- define "recollect.labels" -}}
app.kubernetes.io/name: {{ include "recollect.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion }}
{{- end -}}
