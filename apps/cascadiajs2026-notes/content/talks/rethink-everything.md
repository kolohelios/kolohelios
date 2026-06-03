Building software is wildly different now because of AI — the same way the cloud changed everything before it. The same movie, playing again.

## The cloud already ran this play

**Before the cloud**, building was capital-intensive: you had to predict the future, and experimentation was expensive. **After**, you start small and scale up, experimentation is cheap, and you don't need God-tier infra devs — Amazon-tier hardware without an Amazon-tier budget. AI is that movie again: **Amazon-scale software without Amazon-scale SWE teams.** The expertise required drops; the things that were rigid aren't anymore.

## Rethink the assumptions we stopped questioning

Why can't we commit `.env` files? Why can't a public repo have private parts? Why can't a file be in two folders at once? Why do we need a file system to compile my app? Why do we assume our codebases still matter — our packages? Why can't we rebuild everything? **Why can't we boil the ocean?**

`Salesforce` is the opposite of all this — a giant pile of features. Everyone needs a few, the Fortune 500 needs more, and there's a long tail that 1% of users touch. You have to build for every snowflake. Or do you? **Breadth is now trivial to cover; depth isn't the problem** — build a platform and let the client's agents handle the depth. Too broad is hard; too deep isn't good.

## Boil the ocean

**Before**: 40 hours to build, 3 hours to deploy. **After**: 30 minutes to build, 3 hours to deploy — now the deploy is the obnoxious part. `shoo.dev` does auth in two lines (easy, not necessarily secure or good); it didn't ship because it was built too deep in the vertical. So make all the problems go away. Theo went too far and built `Lakebed` [alpha] — `npx lakebed new sup-seattle`, then `npx lakebed deploy`, deployed in seconds; change something, redeploy in seconds. He built the language, compiler, CLI, *and* deployer. We kid ourselves that we need all the perfect things — just prompt something into existence. `lakebed.dev`.

**Why haven't we been thinking right?** We are not building big enough. Find the place where going so big makes you feel STUPID. The last 5, 10, 50 years don't matter — where will things be in five? You aren't building big enough.
