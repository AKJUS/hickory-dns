// Copyright 2015-2023 Benjamin Fry <benjaminfry@me.com>
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// https://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// https://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! allows a DNS domain name holder to specify one or more Certification
//! Authorities (CAs) authorized to issue certificates for that domain.
//!
//! [RFC 8659, DNS Certification Authority Authorization, November 2019](https://www.rfc-editor.org/rfc/rfc8659)
//!
//! ```text
//! The Certification Authority Authorization (CAA) DNS Resource Record
//! allows a DNS domain name holder to specify one or more Certification
//! Authorities (CAs) authorized to issue certificates for that domain
//! name.  CAA Resource Records allow a public CA to implement additional
//! controls to reduce the risk of unintended certificate mis-issue.
//! This document defines the syntax of the CAA record and rules for
//! processing CAA records by CAs.
//! ```
#![allow(clippy::use_self)]

use alloc::{borrow::ToOwned, string::String, vec::Vec};
use core::{fmt, str};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    error::{ProtoError, ProtoResult},
    rr::{RData, RecordData, RecordDataDecodable, RecordType, domain::Name},
    serialize::binary::*,
};

/// The CAA RR Type
///
/// [RFC 8659, DNS Certification Authority Authorization, November 2019](https://www.rfc-editor.org/rfc/rfc8659)
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct CAA {
    pub(crate) issuer_critical: bool,
    pub(crate) reserved_flags: u8,
    pub(crate) raw_tag: String,
    pub(crate) raw_value: Vec<u8>,
}

impl CAA {
    fn issue(
        issuer_critical: bool,
        tag: IssueProperty,
        name: Option<Name>,
        options: Vec<KeyValue>,
    ) -> Self {
        let raw_tag = tag.as_str().to_owned();
        let raw_value = encode_issuer_value(name.as_ref(), &options);

        Self {
            issuer_critical,
            reserved_flags: 0,
            raw_tag,
            raw_value,
        }
    }

    /// Creates a new CAA issue record data, the tag is `issue`
    ///
    /// # Arguments
    ///
    /// * `issuer_critical` - indicates that the corresponding property tag MUST be understood if the semantics of the CAA record are to be correctly interpreted by an issuer
    /// * `name` - authorized to issue certificates for the associated record label
    /// * `options` - additional options for the issuer, e.g. 'account', etc.
    pub fn new_issue(issuer_critical: bool, name: Option<Name>, options: Vec<KeyValue>) -> Self {
        Self::issue(issuer_critical, IssueProperty::Issue, name, options)
    }

    /// Creates a new CAA issue record data, the tag is `issuewild`
    ///
    /// # Arguments
    ///
    /// * `issuer_critical` - indicates that the corresponding property tag MUST be understood if the semantics of the CAA record are to be correctly interpreted by an issuer
    /// * `name` - authorized to issue certificates for the associated record label
    /// * `options` - additional options for the issuer, e.g. 'account', etc.
    pub fn new_issuewild(
        issuer_critical: bool,
        name: Option<Name>,
        options: Vec<KeyValue>,
    ) -> Self {
        Self::issue(issuer_critical, IssueProperty::IssueWild, name, options)
    }

    /// Creates a new CAA issue record data, the tag is `iodef`
    ///
    /// # Arguments
    ///
    /// * `issuer_critical` - indicates that the corresponding property tag MUST be understood if the semantics of the CAA record are to be correctly interpreted by an issuer
    /// * `url` - Url where issuer errors should be reported
    pub fn new_iodef(issuer_critical: bool, url: Url) -> Self {
        let raw_value = url.as_str().as_bytes().to_vec();
        Self {
            issuer_critical,
            reserved_flags: 0,
            raw_tag: "iodef".to_owned(),
            raw_value,
        }
    }

    /// Indicates that the corresponding property tag MUST be understood if the semantics of the CAA record are to be correctly interpreted by an issuer
    pub fn issuer_critical(&self) -> bool {
        self.issuer_critical
    }

    /// Set the Issuer Critical Flag. This indicates that the corresponding property tag MUST be
    /// understood if the semantics of the CAA record are to be correctly interpreted by an issuer.
    pub fn set_issuer_critical(&mut self, issuer_critical: bool) {
        self.issuer_critical = issuer_critical;
    }

    /// Returns the Flags field of the resource record
    pub fn flags(&self) -> u8 {
        let mut flags = self.reserved_flags & 0b0111_1111;
        if self.issuer_critical {
            flags |= 0b1000_0000;
        }
        flags
    }

    /// The property tag, see struct documentation
    pub fn tag(&self) -> &str {
        &self.raw_tag
    }

    /// Set the property tag, see struct documentation
    pub fn set_tag(&mut self, tag: String) {
        self.raw_tag = tag;
    }

    /// Set the value associated with an `issue` or `issuewild` tag.
    ///
    /// This returns an error if the tag is not `issue` or `issuewild`.
    pub fn set_issuer_value(
        &mut self,
        name: Option<&Name>,
        key_values: &[KeyValue],
    ) -> ProtoResult<()> {
        if !self.raw_tag.eq_ignore_ascii_case("issue")
            && !self.raw_tag.eq_ignore_ascii_case("issuewild")
        {
            return Err("CAA property tag is not 'issue' or 'issuewild'".into());
        }
        self.raw_value = encode_issuer_value(name, key_values);
        Ok(())
    }

    /// Set the value associated with an `iodef` tag.
    ///
    /// This returns an error if the tag is not `iodef`.
    pub fn set_iodef_value(&mut self, url: &Url) -> ProtoResult<()> {
        if !self.raw_tag.eq_ignore_ascii_case("iodef") {
            return Err("CAA property tag is not 'iodef'".into());
        }
        self.raw_value = url.as_str().as_bytes().to_vec();
        Ok(())
    }

    /// Get the value of an `issue` or `issuewild` CAA record.
    ///
    /// This returns an error if the record's tag is not `issue` or `issuewild`, or if the value
    /// does not match the expected syntax.
    pub fn value_as_issue(&self) -> ProtoResult<(Option<Name>, Vec<KeyValue>)> {
        if !self.raw_tag.eq_ignore_ascii_case("issue")
            && !self.raw_tag.eq_ignore_ascii_case("issuewild")
        {
            return Err("CAA property tag is not 'issue' or 'issuewild'".into());
        }
        read_issuer(&self.raw_value)
    }

    /// Get the value of an `iodef` CAA record.
    ///
    /// This returns an error if the record's tag is not `iodef`, or if the value is an invalid URL.
    pub fn value_as_iodef(&self) -> ProtoResult<Url> {
        if !self.raw_tag.eq_ignore_ascii_case("iodef") {
            return Err("CAA property tag is not 'iodef'".into());
        }
        read_iodef(&self.raw_value)
    }

    /// Get the raw value of the CAA record.
    pub fn raw_value(&self) -> &[u8] {
        &self.raw_value
    }
}

enum IssueProperty {
    Issue,
    IssueWild,
}

impl IssueProperty {
    fn as_str(&self) -> &str {
        match self {
            Self::Issue => "issue",
            Self::IssueWild => "issuewild",
        }
    }
}

fn encode_issuer_value(name: Option<&Name>, key_values: &[KeyValue]) -> Vec<u8> {
    let mut output = Vec::new();

    // output the name
    if let Some(name) = name {
        let name = name.to_ascii();
        output.extend_from_slice(name.as_bytes());
    }

    // if there was no name, then we just output ';'
    if name.is_none() && key_values.is_empty() {
        output.push(b';');
        return output;
    }

    for key_value in key_values {
        output.push(b';');
        output.push(b' ');
        output.extend_from_slice(key_value.key.as_bytes());
        output.push(b'=');
        output.extend_from_slice(key_value.value.as_bytes());
    }

    output
}

enum ParseNameKeyPairState {
    BeforeKey(Vec<KeyValue>),
    Key {
        first_char: bool,
        key: String,
        key_values: Vec<KeyValue>,
    },
    Value {
        key: String,
        value: String,
        key_values: Vec<KeyValue>,
    },
}

/// Reads the issuer field according to the spec
///
/// [RFC 8659, DNS Certification Authority Authorization, November 2019](https://www.rfc-editor.org/rfc/rfc8659),
/// and [errata 7139](https://www.rfc-editor.org/errata/eid7139)
///
/// ```text
/// 4.2.  CAA issue Property
///
///    If the issue Property Tag is present in the Relevant RRset for an
///    FQDN, it is a request that Issuers:
///
///    1.  Perform CAA issue restriction processing for the FQDN, and
///
///    2.  Grant authorization to issue certificates containing that FQDN to
///        the holder of the issuer-domain-name or a party acting under the
///        explicit authority of the holder of the issuer-domain-name.
///
///    The CAA issue Property Value has the following sub-syntax (specified
///    in ABNF as per [RFC5234]).
///
///    issue-value = *WSP [issuer-domain-name *WSP]
///       [";" *WSP [parameters *WSP]]
///
///    issuer-domain-name = label *("." label)
///    label = (ALPHA / DIGIT) *( *("-") (ALPHA / DIGIT))
///
///    parameters = (parameter *WSP ";" *WSP parameters) / parameter
///    parameter = parameter-tag *WSP "=" *WSP parameter-value
///    parameter-tag = (ALPHA / DIGIT) *( *("-") (ALPHA / DIGIT))
///    parameter-value = *(%x21-3A / %x3C-7E)
///
///    For consistency with other aspects of DNS administration, FQDN values
///    are specified in letter-digit-hyphen Label (LDH-Label) form.
///
///    The following CAA RRset requests that no certificates be issued for
///    the FQDN "certs.example.com" by any Issuer other than ca1.example.net
///    or ca2.example.org.
///
///    certs.example.com         CAA 0 issue "ca1.example.net"
///    certs.example.com         CAA 0 issue "ca2.example.org"
///
///    Because the presence of an issue Property Tag in the Relevant RRset
///    for an FQDN restricts issuance, FQDN owners can use an issue Property
///    Tag with no issuer-domain-name to request no issuance.
///
///    For example, the following RRset requests that no certificates be
///    issued for the FQDN "nocerts.example.com" by any Issuer.
///
///    nocerts.example.com       CAA 0 issue ";"
///
///    An issue Property Tag where the issue-value does not match the ABNF
///    grammar MUST be treated the same as one specifying an empty
///    issuer-domain-name.  For example, the following malformed CAA RRset
///    forbids issuance:
///
///    malformed.example.com     CAA 0 issue "%%%%%"
///
///    CAA authorizations are additive; thus, the result of specifying both
///    an empty issuer-domain-name and a non-empty issuer-domain-name is the
///    same as specifying just the non-empty issuer-domain-name.
///
///    An Issuer MAY choose to specify parameters that further constrain the
///    issue of certificates by that Issuer -- for example, specifying that
///    certificates are to be subject to specific validation policies,
///    billed to certain accounts, or issued under specific trust anchors.
///
///    For example, if ca1.example.net has requested that its customer
///    account.example.com specify their account number "230123" in each of
///    the customer's CAA records using the (CA-defined) "account"
///    parameter, it would look like this:
///
///    account.example.com   CAA 0 issue "ca1.example.net; account=230123"
///
///    The semantics of parameters to the issue Property Tag are determined
///    by the Issuer alone.
/// ```
///
/// Updated parsing rules:
///
/// [RFC8659 Canonical presentation form and ABNF](https://www.rfc-editor.org/rfc/rfc8659#name-canonical-presentation-form)
///
/// This explicitly allows `-` in property tags, diverging from the original RFC. To support this,
/// property tags will allow `-` as non-starting characters. Additionally, this significantly
/// relaxes the characters allowed in the value to allow URL like characters (it does not validate
/// URL syntax).
pub fn read_issuer(bytes: &[u8]) -> ProtoResult<(Option<Name>, Vec<KeyValue>)> {
    let mut byte_iter = bytes.iter();

    // we want to reuse the name parsing rules
    let name: Option<Name> = {
        let take_name = byte_iter.by_ref().take_while(|ch| char::from(**ch) != ';');
        let name_str = take_name.cloned().collect::<Vec<u8>>();

        if !name_str.is_empty() {
            let name_str = str::from_utf8(&name_str)?;
            Some(Name::from_ascii(name_str)?)
        } else {
            None
        }
    };

    // initial state is looking for a key ';' is valid...
    let mut state = ParseNameKeyPairState::BeforeKey(vec![]);

    // run the state machine through all remaining data, collecting all parameter tag/value pairs.
    for ch in byte_iter {
        match state {
            // Name was already successfully parsed, otherwise we couldn't get here.
            ParseNameKeyPairState::BeforeKey(key_values) => {
                match char::from(*ch) {
                    // gobble ';', ' ', and tab
                    ';' | ' ' | '\u{0009}' => state = ParseNameKeyPairState::BeforeKey(key_values),
                    ch if ch.is_ascii_alphanumeric() && ch != '=' => {
                        // We found the beginning of a new Key
                        let mut key = String::new();
                        key.push(ch);

                        state = ParseNameKeyPairState::Key {
                            first_char: true,
                            key,
                            key_values,
                        }
                    }
                    ch => return Err(format!("bad character in CAA issuer key: {ch}").into()),
                }
            }
            ParseNameKeyPairState::Key {
                first_char,
                mut key,
                key_values,
            } => {
                match char::from(*ch) {
                    // transition to value
                    '=' => {
                        let value = String::new();
                        state = ParseNameKeyPairState::Value {
                            key,
                            value,
                            key_values,
                        }
                    }
                    // push onto the existing key
                    ch if (ch.is_ascii_alphanumeric() || (!first_char && ch == '-'))
                        && ch != '='
                        && ch != ';' =>
                    {
                        key.push(ch);
                        state = ParseNameKeyPairState::Key {
                            first_char: false,
                            key,
                            key_values,
                        }
                    }
                    ch => return Err(format!("bad character in CAA issuer key: {ch}").into()),
                }
            }
            ParseNameKeyPairState::Value {
                key,
                mut value,
                mut key_values,
            } => {
                match char::from(*ch) {
                    // transition back to find another pair
                    ';' => {
                        key_values.push(KeyValue { key, value });
                        state = ParseNameKeyPairState::BeforeKey(key_values);
                    }
                    // If the next byte is a visible character, excluding ';', push it onto the
                    // existing value. See the ABNF production rule for `parameter-value` in the
                    // documentation above.
                    ch if ('\x21'..='\x3A').contains(&ch) || ('\x3C'..='\x7E').contains(&ch) => {
                        value.push(ch);

                        state = ParseNameKeyPairState::Value {
                            key,
                            value,
                            key_values,
                        }
                    }
                    ch => return Err(format!("bad character in CAA issuer value: '{ch}'").into()),
                }
            }
        }
    }

    // valid final states are BeforeKey, where there was a final ';' but nothing followed it.
    //                        Value, where we collected the final chars of the value, but no more data
    let key_values = match state {
        ParseNameKeyPairState::BeforeKey(key_values) => key_values,
        ParseNameKeyPairState::Value {
            key,
            value,
            mut key_values,
        } => {
            key_values.push(KeyValue { key, value });
            key_values
        }
        ParseNameKeyPairState::Key { key, .. } => {
            return Err(format!("key missing value: {key}").into());
        }
    };

    Ok((name, key_values))
}

/// Incident Object Description Exchange Format
///
/// [RFC 8659, DNS Certification Authority Authorization, November 2019](https://www.rfc-editor.org/rfc/rfc8659#section-4.4)
///
/// ```text
/// 4.4.  CAA iodef Property
///
///    The iodef Property specifies a means of reporting certificate issue
///    requests or cases of certificate issue for domains for which the
///    Property appears in the Relevant RRset, when those requests or
///    issuances violate the security policy of the Issuer or the FQDN
///    holder.
///
///    The Incident Object Description Exchange Format (IODEF) [RFC7970] is
///    used to present the incident report in machine-readable form.
///
///    The iodef Property Tag takes a URL as its Property Value.  The URL
///    scheme type determines the method used for reporting:
///
///    mailto:  The IODEF report is reported as a MIME email attachment to
///       an SMTP email that is submitted to the mail address specified.
///       The mail message sent SHOULD contain a brief text message to alert
///       the recipient to the nature of the attachment.
///
///    http or https:  The IODEF report is submitted as a web service
///       request to the HTTP address specified using the protocol specified
///       in [RFC6546].
///
///    These are the only supported URL schemes.
///
///    The following RRset specifies that reports may be made by means of
///    email with the IODEF data as an attachment, a web service [RFC6546],
///    or both:
///
///    report.example.com         CAA 0 issue "ca1.example.net"
///    report.example.com         CAA 0 iodef "mailto:security@example.com"
///    report.example.com         CAA 0 iodef "https://iodef.example.com/"
/// ```
pub fn read_iodef(url: &[u8]) -> ProtoResult<Url> {
    let url = str::from_utf8(url)?;
    let url = Url::parse(url)?;
    Ok(url)
}

/// Issuer parameter key-value pairs.
///
/// [RFC 8659, DNS Certification Authority Authorization, November 2019](https://www.rfc-editor.org/rfc/rfc8659#section-4.2)
/// for more explanation.
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct KeyValue {
    key: String,
    value: String,
}

impl KeyValue {
    /// Construct a new KeyValue pair
    pub fn new<K: Into<String>, V: Into<String>>(key: K, value: V) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    /// Gets a reference to the key of the pair.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Gets a reference to the value of the pair.
    pub fn value(&self) -> &str {
        &self.value
    }
}

// TODO: change this to return &str
fn read_tag(decoder: &mut BinDecoder<'_>, len: Restrict<u8>) -> ProtoResult<String> {
    let len = len
        .map(|len| len as usize)
        .verify_unwrap(|len| *len > 0 && *len <= 15)
        .map_err(|_| ProtoError::from("CAA tag length out of bounds, 1-15"))?;
    let mut tag = String::with_capacity(len);

    for _ in 0..len {
        let ch = decoder
            .pop()?
            .map(char::from)
            .verify_unwrap(|ch| matches!(ch, 'a'..='z' | 'A'..='Z' | '0'..='9'))
            .map_err(|_| ProtoError::from("CAA tag character(s) out of bounds"))?;

        tag.push(ch);
    }

    Ok(tag)
}

/// writes out the tag in binary form to the buffer, returning the number of bytes written
fn emit_tag(buf: &mut [u8], tag: &str) -> ProtoResult<u8> {
    let len = tag.len();
    if len > u8::MAX as usize {
        return Err(format!("CAA property too long: {len}").into());
    }
    if buf.len() < len {
        return Err(format!(
            "insufficient capacity in CAA buffer: {} for tag: {}",
            buf.len(),
            len
        )
        .into());
    }

    // copy into the buffer
    let buf = &mut buf[0..len];
    buf.copy_from_slice(tag.as_bytes());

    Ok(len as u8)
}

impl BinEncodable for CAA {
    fn emit(&self, encoder: &mut BinEncoder<'_>) -> ProtoResult<()> {
        let mut encoder = encoder.with_rdata_behavior(RDataEncoding::Other);
        encoder.emit(self.flags())?;
        // TODO: it might be interesting to use the new place semantics here to output all the data, then place the length back to the beginning...
        let mut tag_buf = [0_u8; u8::MAX as usize];
        let len = emit_tag(&mut tag_buf, &self.raw_tag)?;

        // now write to the encoder
        encoder.emit(len)?;
        encoder.emit_vec(&tag_buf[0..len as usize])?;
        encoder.emit_vec(&self.raw_value)?;

        Ok(())
    }
}

impl<'r> RecordDataDecodable<'r> for CAA {
    /// Read the binary CAA format
    ///
    /// [RFC 8659, DNS Certification Authority Authorization, November 2019](https://www.rfc-editor.org/rfc/rfc8659#section-4.1)
    ///
    /// ```text
    /// 4.1.  Syntax
    ///
    /// A CAA RR contains a single Property consisting of a tag-value pair.
    /// An FQDN MAY have multiple CAA RRs associated with it, and a given
    /// Property Tag MAY be specified more than once across those RRs.
    ///
    /// The RDATA section for a CAA RR contains one Property.  A Property
    /// consists of the following:
    ///
    /// +0-1-2-3-4-5-6-7-|0-1-2-3-4-5-6-7-|
    /// | Flags          | Tag Length = n |
    /// +----------------|----------------+...+---------------+
    /// | Tag char 0     | Tag char 1     |...| Tag char n-1  |
    /// +----------------|----------------+...+---------------+
    /// +----------------|----------------+.....+----------------+
    /// | Value byte 0   | Value byte 1   |.....| Value byte m-1 |
    /// +----------------|----------------+.....+----------------+
    ///
    /// Where n is the length specified in the Tag Length field and m is the
    /// number of remaining octets in the Value field.  They are related by
    /// (m = d - n - 2) where d is the length of the RDATA section.
    ///
    /// The fields are defined as follows:
    ///
    /// Flags:  One octet containing the following field:
    ///
    ///    Bit 0, Issuer Critical Flag:  If the value is set to "1", the
    ///       Property is critical.  A CA MUST NOT issue certificates for any
    ///       FQDN if the Relevant RRset for that FQDN contains a CAA
    ///       critical Property for an unknown or unsupported Property Tag.
    ///
    /// Note that according to the conventions set out in [RFC1035], bit 0 is
    /// the Most Significant Bit and bit 7 is the Least Significant Bit.
    /// Thus, according to those conventions, the Flags value 1 means that
    /// bit 7 is set, while a value of 128 means that bit 0 is set.
    ///
    /// All other bit positions are reserved for future use.
    ///
    /// To ensure compatibility with future extensions to CAA, DNS records
    /// compliant with this version of the CAA specification MUST clear (set
    /// to "0") all reserved flag bits.  Applications that interpret CAA
    /// records MUST ignore the value of all reserved flag bits.
    ///
    /// Tag Length:  A single octet containing an unsigned integer specifying
    ///    the tag length in octets.  The tag length MUST be at least 1.
    ///
    /// Tag:  The Property identifier -- a sequence of ASCII characters.
    ///
    /// Tags MAY contain ASCII characters "a" through "z", "A" through "Z",
    /// and the numbers 0 through 9.  Tags MUST NOT contain any other
    /// characters.  Matching of tags is case insensitive.
    ///
    /// Tags submitted for registration by IANA MUST NOT contain any
    /// characters other than the (lowercase) ASCII characters "a" through
    /// "z" and the numbers 0 through 9.
    ///
    /// Value:  A sequence of octets representing the Property Value.
    ///    Property Values are encoded as binary values and MAY employ
    ///    sub-formats.
    ///
    /// The length of the Value field is specified implicitly as the
    /// remaining length of the enclosing RDATA section.
    /// ```
    fn read_data(decoder: &mut BinDecoder<'r>, length: Restrict<u16>) -> ProtoResult<CAA> {
        let flags = decoder.read_u8()?.unverified(/*used as bitfield*/);

        let issuer_critical = (flags & 0b1000_0000) != 0;
        let reserved_flags = flags & 0b0111_1111;

        let tag_len = decoder.read_u8()?;
        let value_len = length
            .checked_sub(u16::from(tag_len.unverified(/*safe usage here*/)))
            .checked_sub(2)
            .map_err(|_| ProtoError::from("CAA tag character(s) out of bounds"))?
            .unverified(/* used only as length safely */);

        let raw_tag = read_tag(decoder, tag_len)?;

        let raw_value =
            decoder.read_vec(value_len as usize)?.unverified(/* stored as uninterpreted data */);

        Ok(CAA {
            issuer_critical,
            reserved_flags,
            raw_tag,
            raw_value,
        })
    }
}

impl RecordData for CAA {
    fn try_borrow(data: &RData) -> Option<&Self> {
        match data {
            RData::CAA(csync) => Some(csync),
            _ => None,
        }
    }

    fn record_type(&self) -> RecordType {
        RecordType::CAA
    }

    fn into_rdata(self) -> RData {
        RData::CAA(self)
    }
}

// FIXME: this needs to be verified to be correct, add tests...
impl fmt::Display for CAA {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            f,
            "{flags} {tag} \"{value}\"",
            flags = self.flags(),
            tag = &self.raw_tag,
            value = String::from_utf8_lossy(&self.raw_value)
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::dbg_macro, clippy::print_stdout)]

    use alloc::{str, string::ToString};
    #[cfg(feature = "std")]
    use std::println;

    use super::*;

    #[test]
    fn test_read_tag() {
        let ok_under15 = b"abcxyzABCXYZ019";
        let mut decoder = BinDecoder::new(ok_under15);

        let read = read_tag(&mut decoder, Restrict::new(ok_under15.len() as u8))
            .expect("failed to read tag");

        assert_eq!(str::from_utf8(ok_under15).unwrap(), read);
    }

    #[test]
    fn test_bad_tag() {
        let bad_under15 = b"-";
        let mut decoder = BinDecoder::new(bad_under15);

        assert!(read_tag(&mut decoder, Restrict::new(bad_under15.len() as u8)).is_err());
    }

    #[test]
    fn test_too_short_tag() {
        let too_short = b"";
        let mut decoder = BinDecoder::new(too_short);

        assert!(read_tag(&mut decoder, Restrict::new(too_short.len() as u8)).is_err());
    }

    #[test]
    fn test_too_long_tag() {
        let too_long = b"0123456789abcdef";
        let mut decoder = BinDecoder::new(too_long);

        assert!(read_tag(&mut decoder, Restrict::new(too_long.len() as u8)).is_err());
    }

    #[test]
    fn test_read_issuer() {
        // (Option<Name>, Vec<KeyValue>)
        assert_eq!(
            read_issuer(b"ca.example.net; account=230123").unwrap(),
            (
                Some(Name::parse("ca.example.net", None).unwrap()),
                vec![KeyValue {
                    key: "account".to_string(),
                    value: "230123".to_string(),
                }],
            )
        );

        assert_eq!(
            read_issuer(b"ca.example.net").unwrap(),
            (Some(Name::parse("ca.example.net", None,).unwrap(),), vec![],)
        );
        assert_eq!(
            read_issuer(b"ca.example.net; policy=ev").unwrap(),
            (
                Some(Name::parse("ca.example.net", None).unwrap(),),
                vec![KeyValue {
                    key: "policy".to_string(),
                    value: "ev".to_string(),
                }],
            )
        );
        assert_eq!(
            read_issuer(b"ca.example.net; account=230123; policy=ev").unwrap(),
            (
                Some(Name::parse("ca.example.net", None).unwrap(),),
                vec![
                    KeyValue {
                        key: "account".to_string(),
                        value: "230123".to_string(),
                    },
                    KeyValue {
                        key: "policy".to_string(),
                        value: "ev".to_string(),
                    },
                ],
            )
        );
        assert_eq!(
            read_issuer(b"example.net; account-uri=https://example.net/account/1234; validation-methods=dns-01").unwrap(),
            (
                Some(Name::parse("example.net", None).unwrap(),),
                vec![
                    KeyValue {
                        key: "account-uri".to_string(),
                        value: "https://example.net/account/1234".to_string(),
                    },
                    KeyValue {
                        key: "validation-methods".to_string(),
                        value: "dns-01".to_string(),
                    },
                ],
            )
        );
        assert_eq!(read_issuer(b";").unwrap(), (None, vec![]));
        read_issuer(b"example.com; param=\xff").unwrap_err();
    }

    #[test]
    fn test_read_iodef() {
        assert_eq!(
            read_iodef(b"mailto:security@example.com").unwrap(),
            Url::parse("mailto:security@example.com").unwrap()
        );
        assert_eq!(
            read_iodef(b"https://iodef.example.com/").unwrap(),
            Url::parse("https://iodef.example.com/").unwrap()
        );
    }

    fn test_encode_decode(rdata: CAA) {
        let mut bytes = Vec::new();
        let mut encoder: BinEncoder<'_> = BinEncoder::new(&mut bytes);
        rdata.emit(&mut encoder).expect("failed to emit caa");
        let bytes = encoder.into_bytes();

        #[cfg(feature = "std")]
        println!("bytes: {bytes:?}");

        let mut decoder: BinDecoder<'_> = BinDecoder::new(bytes);
        let read_rdata = CAA::read_data(&mut decoder, Restrict::new(bytes.len() as u16))
            .expect("failed to read back");
        assert_eq!(rdata, read_rdata);
    }

    #[test]
    fn test_encode_decode_issue() {
        test_encode_decode(CAA::new_issue(true, None, vec![]));
        test_encode_decode(CAA::new_issue(
            true,
            Some(Name::parse("example.com", None).unwrap()),
            vec![],
        ));
        test_encode_decode(CAA::new_issue(
            true,
            Some(Name::parse("example.com", None).unwrap()),
            vec![KeyValue::new("key", "value")],
        ));
        // technically the this parser supports this case, though it's not clear it's something the spec allows for
        test_encode_decode(CAA::new_issue(
            true,
            None,
            vec![KeyValue::new("key", "value")],
        ));
        // test fqdn
        test_encode_decode(CAA::new_issue(
            true,
            Some(Name::parse("example.com.", None).unwrap()),
            vec![],
        ));
        // invalid name
        test_encode_decode(CAA {
            issuer_critical: false,
            reserved_flags: 0,
            raw_tag: "issue".to_string(),
            raw_value: b"%%%%%".to_vec(),
        });
    }

    #[test]
    fn test_encode_decode_issuewild() {
        test_encode_decode(CAA::new_issuewild(false, None, vec![]));
        // other variants handled in test_encode_decode_issue
    }

    #[test]
    fn test_encode_decode_iodef() {
        test_encode_decode(CAA::new_iodef(
            true,
            Url::parse("https://www.example.com").unwrap(),
        ));
        test_encode_decode(CAA::new_iodef(
            false,
            Url::parse("mailto:root@example.com").unwrap(),
        ));
        // invalid UTF-8
        test_encode_decode(CAA {
            issuer_critical: false,
            reserved_flags: 0,
            raw_tag: "iodef".to_string(),
            raw_value: vec![0xff],
        });
    }

    #[test]
    fn test_encode_decode_unknown() {
        test_encode_decode(CAA {
            issuer_critical: true,
            reserved_flags: 0,
            raw_tag: "tbs".to_string(),
            raw_value: b"Unknown".to_vec(),
        });
    }

    fn test_encode(rdata: CAA, encoded: &[u8]) {
        let mut bytes = Vec::new();
        let mut encoder: BinEncoder<'_> = BinEncoder::new(&mut bytes);
        rdata.emit(&mut encoder).expect("failed to emit caa");
        let bytes = encoder.into_bytes();
        assert_eq!(bytes as &[u8], encoded);
    }

    #[test]
    fn test_encode_non_fqdn() {
        let name_bytes: &[u8] = b"issueexample.com";
        let header: &[u8] = &[128, 5];
        let encoded: Vec<u8> = header.iter().chain(name_bytes.iter()).cloned().collect();

        test_encode(
            CAA::new_issue(
                true,
                Some(Name::parse("example.com", None).unwrap()),
                vec![],
            ),
            &encoded,
        );
    }

    #[test]
    fn test_encode_fqdn() {
        let name_bytes: &[u8] = b"issueexample.com.";
        let header: [u8; 2] = [128, 5];
        let encoded: Vec<u8> = header.iter().chain(name_bytes.iter()).cloned().collect();

        test_encode(
            CAA::new_issue(
                true,
                Some(Name::parse("example.com.", None).unwrap()),
                vec![],
            ),
            &encoded,
        );
    }

    #[test]
    fn test_to_string() {
        let deny = CAA::new_issue(false, None, vec![]);
        assert_eq!(deny.to_string(), "0 issue \";\"");

        let empty_options = CAA::new_issue(
            false,
            Some(Name::parse("example.com", None).unwrap()),
            vec![],
        );
        assert_eq!(empty_options.to_string(), "0 issue \"example.com\"");

        let one_option = CAA::new_issue(
            false,
            Some(Name::parse("example.com", None).unwrap()),
            vec![KeyValue::new("one", "1")],
        );
        assert_eq!(one_option.to_string(), "0 issue \"example.com; one=1\"");

        let two_options = CAA::new_issue(
            false,
            Some(Name::parse("example.com", None).unwrap()),
            vec![KeyValue::new("one", "1"), KeyValue::new("two", "2")],
        );
        assert_eq!(
            two_options.to_string(),
            "0 issue \"example.com; one=1; two=2\""
        );

        let flag_set = CAA::new_issue(
            true,
            Some(Name::parse("example.com", None).unwrap()),
            vec![KeyValue::new("one", "1"), KeyValue::new("two", "2")],
        );
        assert_eq!(
            flag_set.to_string(),
            "128 issue \"example.com; one=1; two=2\""
        );

        let empty_domain = CAA::new_issue(
            false,
            None,
            vec![KeyValue::new("one", "1"), KeyValue::new("two", "2")],
        );
        assert_eq!(empty_domain.to_string(), "0 issue \"; one=1; two=2\"");

        // Examples from RFC 6844, with added quotes
        assert_eq!(
            CAA::new_issue(
                false,
                Some(Name::parse("ca.example.net", None).unwrap()),
                vec![KeyValue::new("account", "230123")]
            )
            .to_string(),
            "0 issue \"ca.example.net; account=230123\""
        );
        assert_eq!(
            CAA::new_issue(
                false,
                Some(Name::parse("ca.example.net", None).unwrap()),
                vec![KeyValue::new("policy", "ev")]
            )
            .to_string(),
            "0 issue \"ca.example.net; policy=ev\""
        );
        assert_eq!(
            CAA::new_iodef(false, Url::parse("mailto:security@example.com").unwrap()).to_string(),
            "0 iodef \"mailto:security@example.com\""
        );
        assert_eq!(
            CAA::new_iodef(false, Url::parse("https://iodef.example.com/").unwrap()).to_string(),
            "0 iodef \"https://iodef.example.com/\""
        );
        let unknown = CAA {
            issuer_critical: true,
            reserved_flags: 0,
            raw_tag: "tbs".to_string(),
            raw_value: b"Unknown".to_vec(),
        };
        assert_eq!(unknown.to_string(), "128 tbs \"Unknown\"");
    }

    #[test]
    fn test_unicode_kv() {
        const MESSAGE: &[u8] = &[
            32, 5, 105, 115, 115, 117, 101, 103, 103, 103, 102, 71, 46, 110, 110, 115, 115, 117,
            48, 110, 45, 59, 32, 32, 255, 61, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        ];

        let mut decoder = BinDecoder::new(MESSAGE);
        let caa = CAA::read_data(&mut decoder, Restrict::new(MESSAGE.len() as u16)).unwrap();
        assert!(!caa.issuer_critical());
        assert_eq!(caa.tag(), "issue");
        match (caa.value_as_issue(), caa.value_as_iodef()) {
            (Err(_), Err(_)) => {}
            _ => panic!("wrong value type"),
        }
        assert_eq!(caa.raw_value, &MESSAGE[7..]);
    }

    #[test]
    fn test_name_non_ascii_character_escaped_dots_roundtrip() {
        const MESSAGE: &[u8] = b"\x00\x05issue\xe5\x85\x9edomain\\.\\.name";
        let caa = CAA::read_data(
            &mut BinDecoder::new(MESSAGE),
            Restrict::new(u16::try_from(MESSAGE.len()).unwrap()),
        )
        .unwrap();

        let mut encoded = Vec::new();
        caa.emit(&mut BinEncoder::new(&mut encoded)).unwrap();

        let caa_round_trip = CAA::read_data(
            &mut BinDecoder::new(&encoded),
            Restrict::new(u16::try_from(encoded.len()).unwrap()),
        )
        .unwrap();

        assert_eq!(caa, caa_round_trip);
    }

    #[test]
    fn test_reserved_flags_round_trip() {
        let mut original = *b"\x00\x05issueexample.com";
        for flags in 0..=u8::MAX {
            original[0] = flags;
            let caa = CAA::read_data(
                &mut BinDecoder::new(&original),
                Restrict::new(u16::try_from(original.len()).unwrap()),
            )
            .unwrap();

            let mut encoded = Vec::new();
            caa.emit(&mut BinEncoder::new(&mut encoded)).unwrap();
            assert_eq!(original.as_slice(), &encoded);
        }
    }
}
