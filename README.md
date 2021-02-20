# Transactions Engine 

## Assumptions
* In the case of a dispute/resolve/chargeback the given client Id matches the client Id of the transaction matching the
given transaction Id. We are not guarding against a mismatch but that could be added if needed.
* In the case of a withdrawal where the funds are insufficient this only fails the particular transaction, but the
engine continues processing subsequent transactions. It is possible we would want to treat this as an error case and
stop processing at least for that particular client.
* In the case of a dispute/resolve/chargeback for an unknown transaction Id we silently ignore it and continue
processing subsequent transactions. We may want to look into adding logging to cover such cases.
* Failure to deserialize a record or process a transaction panic's the program such that no further processing is done.
* Attempting to process a transaction on a locked account silently fails. We may want to treat this as an error case.
* There is an upper bound on the value of an amount such that the fixed decimal precision of 4 decimal points is
maintained for the decimal values used from the `rust_decimal` crate.
transactions if they occur.
* Withdrawals can be disputed/resolved/chargedback in essentially the reverse fashion of a deposit.

## Resource Considerations
### Streaming
* Input is streamed in via the iterator provided by `csv` crate which means we should not be pre-allocating the entire
  input before processing.
* The engine streams out its state on read via an iterator. This is to avoid eagerly allocating a collection for
  example. This was also done because I am copying the data out of the engine so that there is no way a reader can mutate
  the internal state of the engine and also that data read (once iterated) is an immutable snapshot-in-time of the
  engine's state.
### Memory Usage
* The program caches previous transactions and disputes in memory which may use a significant amount of memory in large
data sets. See design considerations for more on this.

## Design Considerations
* Using the `rust_decimal` crate for `Decimal` type to store large amounts of currency with the required precision. I'm
not sure if this is overkill as there may be an easier way to do this, would need to look into it further.
* The engine is currently designed to be interacted with by a single thread in the spirit of KISS. If additional
constraints were added to interact with the engine from multiple threads we would need to look into an appropriate
  locking strategy depending on the nature of how it would be used. For example if the use case involves mostly reads we
  might be able to get away with a simple read-write lock wrapping the engine.
* Valid formatting and bounds of the input is validated by the type system and the deserialization process, invalid
input will halt the program.
* We are caching transactions and disputes in memory as we go to enable us to quickly process dispute/resolve/chargeback
cases without having to read back from the file. This is a time vs space tradeoff. We could instead choose to re-read
from the file or perhaps persist to a database as we read and retrieve from that to keep the records off-heap while
still enabling quick lookup (as we could index it).

## Testing
### Unit Tests
There is unit test coverage for the internals of the `engine` module.

### Manual Tests
There is test input csv files in `/test_data` for testing the binary against in a black-box manner.
