module returns version 1, currency EUR

kind Applicant   { name: Text }
kind Application { applicant: Applicant, paid: Bool = no }
kind Courier     { application: Application }
kind Collection  { application: Application }
kind Locker      { application: Application }

Application needs Return
Return      is    Courier or Collection or Locker

effect send Courier via couriers within 2 days
  sends    the application's applicant's name
  marks    sent
  reports  delivered | fails with lost
effect notify Collection via mailer within 1 hour
  sends    the application's applicant's name
  reports  notified
effect open Locker via lockers within 1 hour
  sends    the application's applicant's name
  reports  opened

rule prepaid: no Application has a Courier that is sent unless it is paid

prefer Return: Courier, then Collection after 3 days, then Locker
decide Return with model "routing@1"
