//! Slice 6: permissions (spec appendix G).

use suture_lang::compile;
use suture_lang::ir::{Admitted, Cond, Num, Op, Path, Program, Step};

fn prog(src: &str) -> Program {
    let files = vec![("t.st".to_string(), format!("module t version 1, currency EUR\n{src}"))];
    compile(&files).unwrap_or_else(|e| panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
}

fn errors(src: &str) -> Vec<String> {
    let files = vec![("t.st".to_string(), format!("module t version 1, currency EUR\n{src}"))];
    compile(&files).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

const SHOP: &str = "role customer is a Customer
role support
role staff
kind Country  { code: Text }
kind Customer { name: Text, country: Country }
kind Product  { name: Text }
kind Order    { customer: Customer, total: Money, cancelled: Bool = no }
admit place a new Order by customer where the Order's customer is the caller
admit add a new Country, Product or Customer by staff
";

#[test]
fn roles_and_admits_compile() {
    let p = prog(SHOP);
    assert_eq!(p.roles.len(), 3);
    assert_eq!(p.role("customer").unwrap().kind.as_deref(), Some("Customer"));
    assert_eq!(p.role("support").unwrap().kind, None);
    assert_eq!(p.admits.len(), 4, "one admit for each kind it names");
    let place = &p.admits[0];
    assert_eq!(place.what, Admitted::New { verb: "place".into(), kind: "Order".into() });
    assert_eq!(place.roles, ["customer"]);
    let want = Cond::Cmp {
        left: Num::Path(Path { steps: vec![Step::Attr("customer".into())] }),
        op: Op::Eq,
        right: Num::Path(Path { steps: vec![Step::Caller] }),
    };
    assert_eq!(place.cond, Some(want));
    assert_eq!(place.source, "place a new Order by customer where the Order's customer is the caller");
    assert_eq!(p.admits[3].what, Admitted::New { verb: "add".into(), kind: "Customer".into() });
}

#[test]
fn the_caller_reads_through_its_unit() {
    let p = prog(&format!("{SHOP}admit add a new Product by customer where the caller's country's code is \"DE\"\n"));
    let Some(Cond::Cmp { left: Num::Path(l), .. }) = &p.admits[4].cond else { panic!("{:?}", p.admits[4].cond) };
    assert_eq!(l.steps, [Step::Caller, Step::Ref("country".into()), Step::Attr("code".into())]);
}

#[test]
fn permission_mistakes_are_errors() {
    let e = errors(&format!("{SHOP}admit place a new Order by guest\n"));
    assert!(e[0].contains("t.st:11") && e[0].contains("there is no role guest; the roles are customer, support, staff"), "{e:?}");
    let e = errors(&format!("{SHOP}admit add a new Product by support where the caller's name is \"x\"\n"));
    assert!(e[0].contains("`the caller's name`: a caller of this admit has a role with no kind"), "{e:?}");
    let e = errors(&format!("{SHOP}admit add a new Product by customer, staff where the caller is the caller\n"));
    assert!(e[0].contains("a role with no kind"), "{e:?}");
    let e = errors(&format!("{SHOP}role staff\n"));
    assert!(e[0].contains("role staff is declared twice"), "{e:?}");
    let e = errors(&format!("{SHOP}role guest is a Visitor\n"));
    assert!(e[0].contains("role guest: there is no kind Visitor"), "{e:?}");
    let e = errors("role Staff\n");
    assert!(e[0].contains("a role is named in lower case"), "{e:?}");
    let e = errors(&format!("{SHOP}kind Payment {{ order: Order }}\nOrder needs Payment\nadmit add a new Payment by staff\n"));
    assert!(e[0].contains("Payment is a part of Order"), "{e:?}");
    let e = errors(&format!("{SHOP}admit add a new Parcel by staff\n"));
    assert!(e[0].contains("there is no kind Parcel"), "{e:?}");
    let e = errors(&format!("{SHOP}admit place a new Order by customer where the caller is \"Ada\"\n"));
    assert!(!e.is_empty(), "the caller is a reference, not text");
}

const EVENTS: &str = "admit cancel Order by customer where the Order's customer is the caller
admit cancel Order by support
when a customer or support cancels an Order: the Order is cancelled
";

#[test]
fn admitted_events_have_reactions() {
    let p = prog(&format!("{SHOP}{EVENTS}"));
    let r = p.reactions.iter().find(|r| r.outcome == "cancels").unwrap();
    assert_eq!((r.kind.as_str(), r.by.clone()), ("Order", vec!["customer".to_string(), "support".to_string()]));
    assert_eq!(r.actions[0].attr, "cancelled");
    assert_eq!(p.admits.iter().filter(|a| matches!(&a.what, Admitted::Event { verb, .. } if verb == "cancel")).count(), 2);
}

#[test]
fn event_mistakes_are_errors() {
    let e = errors(&format!("{SHOP}when a customer cancels an Order: the Order is cancelled\n"));
    assert!(e[0].contains("no `admit` lets anyone cancel an Order; add `admit cancel Order by customer`"), "{e:?}");
    let e = errors(&format!("{SHOP}admit cancel Order by support\nwhen a customer or support cancels an Order: the Order is cancelled\n"));
    assert!(e[0].contains("`cancels`: only support may do this, not customer"), "{e:?}");
    let e = errors(&format!("{SHOP}admit cancel Order by support\n"));
    assert!(e[0].contains("`admit cancel Order by support`: no reaction handles it; add `when a support cancels an Order: …`"), "{e:?}");
    let e = errors(&format!("{SHOP}admit fix Order by support\nwhen a support fixes an Order: the Order is fixed\n"));
    assert!(e[0].contains("no Bool attribute `fixed`"), "{e:?}");
}

const CHANGES: &str = "role staff
kind Country  { code: Text, restricted: Bool = no }
kind Product  { name: Text, price: Money }
kind Stock    { product: Product, units: Count, label: Text = product's name }
kind Order    { country: Country, product: Product, quantity: Count }
kind Shipment { order: Order }
Order needs Shipment
Shipment uses a Stock where the Stock's product is the Order's product, taking the Order's quantity
prefer Stock: highest units
rule stock: the total quantity taken from each Stock is at most its units
rule embargo: no Order whose country is restricted has a Shipment
";

#[test]
fn admitted_changes_make_facts_changeable() {
    let fixed = prog(CHANGES);
    assert!(fixed.rules[1].fixed, "no one changes restricted");
    assert_eq!(fixed.rules[1].tier, "local");
    let p = prog(&format!("{CHANGES}admit change Stock's units by staff\nadmit change Country's restricted by staff\n"));
    assert_eq!(p.admits[0].what, Admitted::Change { kind: "Stock".into(), attr: "units".into() });
    assert!(!p.rules[1].fixed, "a caller can change restricted, so the refusal is not fixed");
    assert_eq!(p.rules[1].tier, "per-group (watches Country's restricted)");
}

#[test]
fn change_mistakes_are_errors() {
    let e = errors(&format!("{CHANGES}admit change Stock's colour by staff\n"));
    assert!(e[0].contains("a Stock has no attribute `colour`"), "{e:?}");
    let e = errors(&format!("{CHANGES}admit change Stock's product by staff\n"));
    assert!(e[0].contains("`product` refers to shared entities"), "{e:?}");
    let e = errors(&format!("{CHANGES}admit change Stock's label by staff\n"));
    assert!(e[0].contains("`label` is calculated when the Stock is created"), "{e:?}");
    let e = errors(&format!("{CHANGES}admit change Stock's created by staff\n"));
    assert!(e[0].contains("`created` is the clock"), "{e:?}");
    let e = errors(&format!("{CHANGES}kind Carrier {{ zone: Text }}\nShipment uses a Carrier where the Carrier's zone is \"EU\"\nprefer Carrier: highest 1\nadmit change Carrier's zone by staff\n"));
    assert!(e.iter().any(|e| e.contains("`zone` keys the pool of `Shipment uses a Carrier`")), "{e:?}");
    let e = errors(&format!("{CHANGES}admit change Shipment's order by staff\n"));
    assert!(e[0].contains("Shipment is a part of Order"), "{e:?}");
    let e = errors(&format!("{CHANGES}admit change Stock by staff\n"));
    assert!(e[0].contains("name the attribute"), "{e:?}");
}
