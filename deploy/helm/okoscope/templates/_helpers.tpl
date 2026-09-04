{{- define "okoscope.fullname" -}}{{ default .Release.Name .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}{{- end }}
{{- define "okoscope.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" }}
app.kubernetes.io/name: okoscope
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}
{{- define "okoscope.image" -}}{{ index . "repository" }}{{ if index . "digest" }}@{{ index . "digest" }}{{ else }}:{{ index . "tag" }}{{ end }}{{- end }}
{{- define "okoscope.internalSecret" -}}{{ default (printf "%s-internal" (include "okoscope.fullname" .)) .Values.internalSecret.existingSecret }}{{- end }}
