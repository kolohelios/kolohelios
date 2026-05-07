package audit

#Severity: "fail" | "off"

#Rule: {
	name:     string & =~"^[a-z][a-z0-9-]*$"
	severity: #Severity
}

#AuditConfig: {
	rules: [...#Rule]
}
