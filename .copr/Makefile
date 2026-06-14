srcname := livesplit-one-druid
commit := $(shell if read head < .git/HEAD && expr "$$head" : 'ref: ' >/dev/null; then ref=$${head#ref: }; if test -f ".git/$$ref"; then read commit < ".git/$$ref"; else commit=$$(sed -n "s|^$$ref ||p" .git/packed-refs); fi; else commit=$$head; fi; printf '%s' "$$commit")
commit_short := $(shell printf '%s' "$(commit)" | cut -c1-7)
date := $(shell date +%Y%m%d)
outdir ?= $(CURDIR)
spec ?= packaging/livesplit-one.spec
archive := $(srcname)-$(commit).tar.gz
generated_spec := $(outdir)/livesplit-one.spec

.PHONY: srpm
srpm:
	printf '%%global commit %s\n' '$(commit)' > $(generated_spec)
	printf '%%global commit_short %s\n' '$(commit_short)' >> $(generated_spec)
	printf '%%global date %s\n\n' '$(date)' >> $(generated_spec)
	cat $(spec) >> $(generated_spec)
	tar --exclude-vcs --exclude='./target' --exclude='*.src.rpm' --exclude='*.tar.gz' --transform 's,^,$(srcname)-$(commit)/,' -czf $(outdir)/$(archive) .
	rpmbuild -bs $(generated_spec) \
		--define "_sourcedir $(outdir)" \
		--define "_srcrpmdir $(outdir)"
