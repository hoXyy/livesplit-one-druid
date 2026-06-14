srcname := livesplit-one-druid
commit := $(shell git rev-parse HEAD)
commit_short := $(shell git rev-parse --short HEAD)
date := $(shell date +%Y%m%d)
archive := $(srcname)-$(commit).tar.gz

.PHONY: srpm
srpm:
	git archive --format=tar.gz --prefix=$(srcname)-$(commit)/ -o $(archive) HEAD
	rpmbuild -bs packaging/livesplit-one.spec \
		--define "_sourcedir $(CURDIR)" \
		--define "_srcrpmdir $(CURDIR)" \
		--define "commit $(commit)" \
		--define "commit_short $(commit_short)" \
		--define "date $(date)"
