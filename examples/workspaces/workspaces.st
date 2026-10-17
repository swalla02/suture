-- Workspace provisioning: a team asks for a workspace with its members,
-- and the system sets it up. Nothing here says in which order.
module workspaces version 1, currency EUR

kind Workspace { team: Text, seats: Count, members: list of Member, rejected: Bool = no,
                 price: Money = seats * 12 EUR }
kind Member    { workspace: Workspace, email: Text, admin: Bool = no }
kind Account   { workspace: Workspace }
kind Repo      { workspace: Workspace }
kind Charge    { workspace: Workspace, amount: Money = workspace's price }
kind Bucket    { workspace: Workspace }
kind Volume    { workspace: Workspace }

Workspace needs Account, Repo, Charge, Storage while it is not rejected
Storage   is    Bucket or Volume

effect open Account via identity within 1 minute
  sends    the workspace's team
  reports  opened | fails with refused
effect create Repo via git within 5 minutes
  sends    the workspace's team
  reports  created
effect bill Charge via billing within 30 seconds
  sends    its amount
  reports  paid | fails with declined
effect make Bucket via storage within 10 minutes
  sends    the workspace's team
  reports  made
effect make Volume via disks within 10 minutes
  sends    the workspace's team
  reports  made

rule seats:   the number of each Workspace's members is at most its seats
rule admin:   every Workspace has a Member that is admin
rule account: no Workspace has a Repo unless its Account is opened

when an Account is refused: its Workspace is rejected
when a Charge is declined:  its Workspace is rejected

define: a Workspace is ready when its Account is opened and its Repo is created and its Charge is paid and its Storage is done

desired within 1 hour: every Workspace that is not rejected is ready

prefer Storage: Bucket, then Volume after 15 minutes
