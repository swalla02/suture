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
