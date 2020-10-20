//! This module implements hash_to_field and related hashing primitives
//! for use with BLS signatures.

use digest::generic_array::{typenum::Unsigned, ArrayLength, GenericArray};
use digest::{BlockInput, Digest, ExtendableOutput, Update};
use std::marker::PhantomData;

/// Hash to field for they type `T` using ExpandMsg variant `X`.
pub fn hash_to_field<T, X>(msg: &[u8], dst: &[u8], count: usize) -> Vec<T>
where
    T: FromRO,
    X: ExpandMsg,
{
    let len_per_elm = <T as FromRO>::Length::to_usize();
    let len_in_bytes = count * len_per_elm;
    let pseudo_random_bytes = X::expand_message(msg, dst, len_in_bytes);

    let mut ret = Vec::<T>::with_capacity(count);
    for idx in 0..count {
        let bytes_to_convert = &pseudo_random_bytes[idx * len_per_elm..(idx + 1) * len_per_elm];
        let bytes_arr = GenericArray::<u8, <T as FromRO>::Length>::from_slice(bytes_to_convert);
        ret.push(T::from_ro(bytes_arr));
    }

    ret
}

/// Generate a field element from a random string of bytes.
pub trait FromRO {
    type Length: ArrayLength<u8>;

    fn from_ro(okm: &GenericArray<u8, <Self as FromRO>::Length>) -> Self;
}

/// BaseFromRO is a FromRO impl for a field with extension degree 1.
impl<T: BaseFromRO> FromRO for T {
    type Length = <T as BaseFromRO>::BaseLength;

    fn from_ro(okm: &GenericArray<u8, <Self as FromRO>::Length>) -> T {
        T::from_okm(okm)
    }
}

/// Generate an element of a base field for a random string of bytes
/// (used by FromRO for extension fields).
pub trait BaseFromRO {
    type BaseLength: ArrayLength<u8>;

    fn from_okm(okm: &GenericArray<u8, <Self as BaseFromRO>::BaseLength>) -> Self;
}

/// Trait for types implementing expand_message interface for hash_to_field
pub trait ExpandMsg {
    fn expand_message(msg: &[u8], dst: &[u8], len_in_bytes: usize) -> Vec<u8>;
}

/// Placeholder type for implementing expand_message_xof based on a hash function
#[derive(Debug)]
pub struct ExpandMsgXof<HashT> {
    phantom: PhantomData<HashT>,
}

/// ExpandMsgXof implements expand_message_xof for the ExpandMsg trait
impl<HashT> ExpandMsg for ExpandMsgXof<HashT>
where
    HashT: Default + ExtendableOutput + Update,
{
    fn expand_message(msg: &[u8], dst: &[u8], len_in_bytes: usize) -> Vec<u8> {
        assert!(
            dst.len() < 256,
            "dst of more than 255bytes is not supported"
        );
        HashT::default()
            .chain(msg)
            .chain([(len_in_bytes >> 8) as u8, len_in_bytes as u8])
            .chain(dst)
            .chain([dst.len() as u8])
            .finalize_boxed(len_in_bytes)
            .to_vec()
    }
}

/// Placeholder type for implementing `expand_message_xmd` based on a hash function.
#[derive(Debug)]
pub struct ExpandMsgXmd<HashT> {
    phantom: PhantomData<HashT>,
}

/// ExpandMsgXmd implements expand_message_xmd for the ExpandMsg trait
impl<HashT> ExpandMsg for ExpandMsgXmd<HashT>
where
    HashT: Digest + BlockInput,
{
    fn expand_message(msg: &[u8], dst: &[u8], len_in_bytes: usize) -> Vec<u8> {
        assert!(
            dst.len() < 256,
            "dst of more than 255bytes is not supported"
        );
        let b_in_bytes = <HashT as Digest>::OutputSize::to_usize();
        let ell = (len_in_bytes + b_in_bytes - 1) / b_in_bytes;
        if ell > 255 {
            panic!("ell was too big in expand_message_xmd");
        }
        let b_0 = HashT::new()
            .chain(GenericArray::<u8, <HashT as BlockInput>::BlockSize>::default())
            .chain(msg)
            .chain([(len_in_bytes >> 8) as u8, len_in_bytes as u8, 0u8])
            .chain(dst)
            .chain([dst.len() as u8])
            .finalize();

        let mut b_vals = Vec::<u8>::with_capacity(ell * b_in_bytes);
        // b_1
        b_vals.extend_from_slice(
            HashT::new()
                .chain(&b_0[..])
                .chain([1u8])
                .chain(dst)
                .chain([dst.len() as u8])
                .finalize()
                .as_ref(),
        );

        for idx in 1..ell {
            // b_0 XOR b_(idx - 1)
            let mut tmp = GenericArray::<u8, <HashT as Digest>::OutputSize>::default();
            b_0.iter()
                .zip(&b_vals[(idx - 1) * b_in_bytes..idx * b_in_bytes])
                .enumerate()
                .for_each(|(jdx, (b0val, bi1val))| tmp[jdx] = b0val ^ bi1val);
            b_vals.extend_from_slice(
                HashT::new()
                    .chain(tmp)
                    .chain([(idx + 1) as u8])
                    .chain(dst)
                    .chain([dst.len() as u8])
                    .finalize()
                    .as_ref(),
            );
        }

        b_vals.truncate(len_in_bytes);
        b_vals
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Sha256, Sha512};
    use sha3::Shake128;

    // Except internal variables, expand_message_xmd and expand_message_xof did not change
    // between draft 7 and draft 8 (https://tools.ietf.org/rfcdiff?difftype=--hwdiff&url2=draft-irtf-cfrg-hash-to-curve-08.txt).
    // So we can use test vector introduced in draft 8 to test the draft 7 implementation.

    /// From https://tools.ietf.org/html/draft-irtf-cfrg-hash-to-curve-08#appendix-I.1
    #[test]
    fn expand_message_xmd_works_for_draft8_testvectors_sha256() {
        let dst = b"QUUX-V01-CS02-with-expander";

        let msg = b"";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("f659819a6473c1835b25ea59e3d38914c98b374f0970b7e4c92181df928fca88")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha256>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"abc";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("1c38f7c211ef233367b2420d04798fa4698080a8901021a795a1151775fe4da7")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha256>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"abcdef0123456789";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("8f7e7b66791f0da0dbb5ec7c22ec637f79758c0a48170bfb7c4611bd304ece89")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha256>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"q128_qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("72d5aa5ec810370d1f0013c0df2f1d65699494ee2a39f72e1716b1b964e1c642")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha256>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"a512_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("3b8e704fc48336aca4c2a12195b720882f2162a4b7b13a9c350db46f429b771b")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha256>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("8bcffd1a3cae24cf9cd7ab85628fd111bb17e3739d3b53f89580d217aa79526f1708354a76a402d3569d6a9d19ef3de4d0b991e4f54b9f20dcde9b95a66824cbdf6c1a963a1913d43fd7ac443a02fc5d9d8d77e2071b86ab114a9f34150954a7531da568a1ea8c760861c0cde2005afc2c114042ee7b5848f5303f0611cf297f")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha256>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"abc";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("fe994ec51bdaa821598047b3121c149b364b178606d5e72bfbb713933acc29c186f316baecf7ea22212f2496ef3f785a27e84a40d8b299cec56032763eceeff4c61bd1fe65ed81decafff4a31d0198619c0aa0c6c51fca15520789925e813dcfd318b542f8799441271f4db9ee3b8092a7a2e8d5b75b73e28fb1ab6b4573c192")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha256>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"abcdef0123456789";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("c9ec7941811b1e19ce98e21db28d22259354d4d0643e301175e2f474e030d32694e9dd5520dde93f3600d8edad94e5c364903088a7228cc9eff685d7eaac50d5a5a8229d083b51de4ccc3733917f4b9535a819b445814890b7029b5de805bf62b33a4dc7e24acdf2c924e9fe50d55a6b832c8c84c7f82474b34e48c6d43867be")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha256>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"q128_qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("48e256ddba722053ba462b2b93351fc966026e6d6db493189798181c5f3feea377b5a6f1d8368d7453faef715f9aecb078cd402cbd548c0e179c4ed1e4c7e5b048e0a39d31817b5b24f50db58bb3720fe96ba53db947842120a068816ac05c159bb5266c63658b4f000cbf87b1209a225def8ef1dca917bcda79a1e42acd8069")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha256>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"a512_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("396962db47f749ec3b5042ce2452b619607f27fd3939ece2746a7614fb83a1d097f554df3927b084e55de92c7871430d6b95c2a13896d8a33bc48587b1f66d21b128a1a8240d5b0c26dfe795a1a842a0807bb148b77c2ef82ed4b6c9f7fcb732e7f94466c8b51e52bf378fba044a31f5cb44583a892f5969dcd73b3fa128816e")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha256>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );
    }

    /// From https://tools.ietf.org/html/draft-irtf-cfrg-hash-to-curve-08#appendix-I.2
    #[test]
    fn expand_message_xmd_works_for_draft8_testvectors_sha512() {
        let dst = b"QUUX-V01-CS02-with-expander";

        let msg = b"";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("2eaa1f7b5715f4736e6a5dbe288257abf1faa028680c1d938cd62ac699ead642")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha512>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"abc";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("0eeda81f69376c80c0f8986496f22f21124cb3c562cf1dc608d2c13005553b0f")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha512>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"abcdef0123456789";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("2e375fc05e05e80dbf3083796fde2911789d9e8847e1fcebf4ca4b36e239b338")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha512>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"q128_qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("c37f9095fe7fe4f01c03c3540c1229e6ac8583b07510085920f62ec66acc0197")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha512>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"a512_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("af57a7f56e9ed2aa88c6eab45c8c6e7638ae02da7c92cc04f6648c874ebd560e")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha512>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("0687ce02eba5eb3faf1c3c539d1f04babd3c0f420edae244eeb2253b6c6d6865145c31458e824b4e87ca61c3442dc7c8c9872b0b7250aa33e0668ccebbd2b386de658ca11a1dcceb51368721ae6dcd2d4bc86eaebc4e0d11fa02ad053289c9b28a03da6c942b2e12c14e88dbde3b0ba619d6214f47212b628f3e1b537b66efcf")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha512>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"abc";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("779ae4fd8a92f365e4df96b9fde97b40486bb005c1a2096c86f55f3d92875d89045fbdbc4a0e9f2d3e1e6bcd870b2d7131d868225b6fe72881a81cc5166b5285393f71d2e68bb0ac603479959370d06bdbe5f0d8bfd9af9494d1e4029bd68ab35a561341dd3f866b3ef0c95c1fdfaab384ce24a23427803dda1db0c7d8d5344a")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha512>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"abcdef0123456789";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("f0953d28846a50e9f88b7ae35b643fc43733c9618751b569a73960c655c068db7b9f044ad5a40d49d91c62302eaa26163c12abfa982e2b5d753049e000adf7630ae117aeb1fb9b61fc724431ac68b369e12a9481b4294384c3c890d576a79264787bc8076e7cdabe50c044130e480501046920ff090c1a091c88391502f0fbac")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha512>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"q128_qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("64d3e59f0bc3c5e653011c914b419ba8310390a9585311fddb26791d26663bd71971c347e1b5e88ba9274d2445ed9dcf48eea9528d807b7952924159b7c27caa4f25a2ea94df9508e70a7012dfce0e8021b37e59ea21b80aa9af7f1a1f2efa4fbe523c4266ce7d342acaacd438e452c501c131156b4945515e9008d2b155c258")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha512>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"a512_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("01524feea5b22f6509f6b1e805c97df94faf4d821b01aadeebc89e9daaed0733b4544e50852fd3e019d58eaad6d267a134c8bc2c08bc46c10bfeff3ee03110bcd8a0d695d75a34092bd8b677bdd369a13325549abab54f4ac907b712bdd3567f38c4554c51902b735b81f43a7ef6f938c7690d107c052c7e7b795ac635b3200a")
                .unwrap();
        assert_eq!(
            ExpandMsgXmd::<Sha512>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );
    }

    /// From https://tools.ietf.org/html/draft-irtf-cfrg-hash-to-curve-08#appendix-I.3
    #[test]
    fn expand_message_xof_works_for_draft8_testvectors_shake128() {
        let dst = b"QUUX-V01-CS02-with-expander";

        let msg = b"";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("eca3fe8f7f5f1d52d7ed3691c321adc7d2a0fef1f843d221f7002530070746de")
                .unwrap();
        assert_eq!(
            ExpandMsgXof::<Shake128>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"abc";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("c79b8ea0af10fd8871eda98334ea9d54e9e5282be97521678f987718b187bc08")
                .unwrap();
        assert_eq!(
            ExpandMsgXof::<Shake128>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"abcdef0123456789";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("fb6f4af2a83f6276e9d41784f1e29da5e27566167c33e5cf2682c30096878b73")
                .unwrap();
        assert_eq!(
            ExpandMsgXof::<Shake128>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"q128_qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("125d05850db915e0683d17d044d87477e6e7b3f70a450dd097761e18d1d1dcdf")
                .unwrap();
        assert_eq!(
            ExpandMsgXof::<Shake128>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"a512_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let len_in_bytes = 0x20;
        let uniform_bytes =
            hex::decode("beafd026cb942c86f6a2b31bb8e6bf7173fb1b0caf3c21ea4b3b9d05d904fd23")
                .unwrap();
        assert_eq!(
            ExpandMsgXof::<Shake128>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("15733b3fb22fac0e0902c220aeea48e5e47d39f36c2cc03eac34367c48f2a3ebbcb3baa8a0cf17ab12fff4defc7ce22aed47188b6c163e828741473bd89cc646a082cb68b8e835b1374ea9a6315d61db0043f4abf506c26386e84668e077c85ebd9d632f4390559b979e70e9e7affbd0ac2a212c03b698efbbe940f2d164732b")
                .unwrap();
        assert_eq!(
            ExpandMsgXof::<Shake128>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"abc";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("4ccafb6d95b91537798d1fbb25b9fbe1a5bbe1683f43a4f6f03ef540b811235317bfc0aefb217faca055e1b8f32dfde9eb102cdc026ed27caa71530e361b3adbb92ccf68da35aed8b9dc7e4e6b5db0666c607a31df05513ddaf4c8ee23b0ee7f395a6e8be32eb13ca97da289f2643616ac30fe9104bb0d3a67a0a525837c2dc6")
                .unwrap();
        assert_eq!(
            ExpandMsgXof::<Shake128>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"abcdef0123456789";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("c8ee0e12736efbc9b47781db9d1e5db9c853684344a6776eb362d75b354f4b74cf60ba1373dc2e22c68efb76a022ed5391f67c77990802018c8cdc7af6d00c86b66a3b3ccad3f18d90f4437a165186f6601cf0bb281ea5d80d1de20fe22bb2e2d8acab0c043e76e3a0f34e0a1e66c9ade4fef9ef3b431130ad6f232babe9fe68")
                .unwrap();
        assert_eq!(
            ExpandMsgXof::<Shake128>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"q128_qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("3eebe6721b2ec746629856dc2dd3f03a830dabfefd7e2d1e72aaf2127d6ad17c988b5762f32e6edf61972378a4106dc4b63fa108ad03b793eedf4588f34c4df2a95b30995a464cb3ee31d6dca30adbfc90ffdf5414d7893082c55b269d9ec9cd6d2a715b9c4fad4eb70ed56f878b55a17b5994ef0de5b338675aad35354195cd")
                .unwrap();
        assert_eq!(
            ExpandMsgXof::<Shake128>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );

        let msg = b"a512_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let len_in_bytes = 0x80;
        let uniform_bytes =
            hex::decode("858cb4a6a5668a97d0f7039b5d6d574dde18dd2323cf6b203945c66df86477d1f747b46401903b3fa66d1276108ea7187b4411b7499acf4600080ce34ff6d21555c2af16f091adf8b285c8439f2e47fa0553c3a6ef5a4227a13f34406241b7d7fd8853a080bad25ec4804cdfe4fda500e1c872e71b8c61a8e160691894b96058")
                .unwrap();
        assert_eq!(
            ExpandMsgXof::<Shake128>::expand_message(msg, dst, len_in_bytes),
            uniform_bytes
        );
    }
}
